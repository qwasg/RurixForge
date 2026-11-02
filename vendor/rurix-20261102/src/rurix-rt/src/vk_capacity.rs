// RFC-0052 HQ capacity query; U30. No allocations, AS creation, or GPU commands.
// Assisted-by: Codex:gpt-6-astra.

pub(crate) struct AsCapacityBufferRequest {
    pub label: String,
    pub size: u64,
    pub usage: u32,
    pub host_visible: bool,
}

impl VkAsManager {
    /// Query the buffer requests used by build_scene before allocating any of them.
    /// Device addresses are ignored by vkGetAccelerationStructureBuildSizesKHR;
    /// transform_data=0 retains the production no-transform-buffer shape.
    ///
    /// # Safety
    /// U30: fns/device belong to a live device with accelerationStructure enabled.
    /// Driver output pointers and geometry pointers live through each query.
    pub(crate) unsafe fn capacity_buffer_requests(
        fns: &VkAsFns,
        device: VkDevice,
        scene: &RayQuerySceneDesc<'_>,
        updatable_blas: &[u32],
        unique_candidates: bool,
    ) -> Result<Vec<AsCapacityBufferRequest>, String> {
        if scene.blas_triangles.is_empty() || scene.instances.is_empty() {
            return Err("AS capacity: empty BLAS or instance table".into());
        }
        if updatable_blas.iter().any(|&i| i as usize >= scene.blas_triangles.len())
            || scene.instances.iter().any(|i| i.blas as usize >= scene.blas_triangles.len())
        {
            return Err("AS capacity: BLAS index out of range".into());
        }
        let storage_usage = BUFFER_USAGE_ACCEL_STRUCTURE_STORAGE | BUFFER_USAGE_SHADER_DEVICE_ADDRESS;
        let scratch_usage = BUFFER_USAGE_STORAGE_BUFFER | BUFFER_USAGE_SHADER_DEVICE_ADDRESS;
        let mut requests = Vec::new();
        let mut query = |label: String, geometry: AccelGeometry, ty: u32, flags: u32,
                         primitive_count: u32, update: bool| -> Result<(), String> {
            let info = AccelBuildGeometryInfo {
                s_type: ST_ACCELERATION_STRUCTURE_BUILD_GEOMETRY_INFO_KHR,
                p_next: std::ptr::null(), ty, flags, mode: BUILD_ACCEL_STRUCTURE_MODE_BUILD,
                src_acceleration_structure: VK_NULL_HANDLE,
                dst_acceleration_structure: VK_NULL_HANDLE,
                geometry_count: 1, p_geometries: &geometry,
                pp_geometries: std::ptr::null(), scratch_data: 0,
            };
            let mut sizes = AccelBuildSizesInfo {
                s_type: ST_ACCELERATION_STRUCTURE_BUILD_SIZES_INFO_KHR,
                p_next: std::ptr::null(), acceleration_structure_size: 0,
                update_scratch_size: 0, build_scratch_size: 0,
            };
            // SAFETY: U30; live enabled device, one live geometry/count, valid output.
            unsafe { (fns.get_as_sizes)(device, ACCEL_STRUCTURE_BUILD_TYPE_DEVICE, &info, &primitive_count, &mut sizes) };
            if sizes.acceleration_structure_size == 0 {
                return Err(format!("AS capacity: {label} returned zero storage size"));
            }
            requests.push(AsCapacityBufferRequest {
                label: format!("{label}.storage"), size: sizes.acceleration_structure_size,
                usage: storage_usage, host_visible: false,
            });
            requests.push(AsCapacityBufferRequest {
                label: format!("{label}.scratch"),
                size: if update { sizes.build_scratch_size.max(sizes.update_scratch_size) } else { sizes.build_scratch_size }.max(4),
                usage: scratch_usage, host_visible: false,
            });
            Ok(())
        };
        // Query all AS sizes first so the closure's borrow ends before vertex rows.
        for (i, tris) in scene.blas_triangles.iter().enumerate() {
            if tris.is_empty() || !tris.len().is_multiple_of(9) {
                return Err(format!("AS capacity: BLAS[{i}] malformed triangle data"));
            }
            let count = u32::try_from(tris.len() / 9).map_err(|_| "AS capacity: primitive count overflow")?;
            let max_vertex = count.checked_mul(3).and_then(|n| n.checked_sub(1))
                .ok_or("AS capacity: maxVertex overflow")?;
            let update = updatable_blas.contains(&(i as u32));
            query(format!("blas[{i}]"), AccelGeometry {
                s_type: ST_ACCELERATION_STRUCTURE_GEOMETRY_KHR,
                p_next: std::ptr::null(), geometry_type: GEOMETRY_TYPE_TRIANGLES,
                geometry: AccelGeometryData { triangles: AccelGeometryTrianglesData {
                    s_type: ST_ACCEL_GEOMETRY_TRIANGLES_DATA_KHR,
                    p_next: std::ptr::null(), vertex_format: FORMAT_R32G32B32_SFLOAT,
                    vertex_data: 0, vertex_stride: 12, max_vertex,
                    index_type: INDEX_TYPE_NONE_KHR, index_data: 0, transform_data: 0,
                }}, flags: triangle_candidate_geometry_flags(unique_candidates),
            }, ACCEL_STRUCTURE_TYPE_BOTTOM_LEVEL,
                if update { BUILD_ACCEL_STRUCTURE_ALLOW_UPDATE_BIT } else { 0 }, count, update)?;
        }
        let instance_count = u32::try_from(scene.instances.len()).map_err(|_| "AS capacity: instance count overflow")?;
        query("tlas".into(), AccelGeometry {
            s_type: ST_ACCELERATION_STRUCTURE_GEOMETRY_KHR,
            p_next: std::ptr::null(), geometry_type: GEOMETRY_TYPE_INSTANCES,
            geometry: AccelGeometryData { instances: AccelGeometryInstancesData {
                s_type: ST_ACCEL_GEOMETRY_INSTANCES_DATA_KHR,
                p_next: std::ptr::null(), array_of_pointers: 0, data: 0,
            }}, flags: GEOMETRY_OPAQUE_BIT,
        }, ACCEL_STRUCTURE_TYPE_TOP_LEVEL, BUILD_ACCEL_STRUCTURE_ALLOW_UPDATE_BIT, instance_count, true)?;
        drop(query);
        for (i, tris) in scene.blas_triangles.iter().enumerate() {
            requests.push(AsCapacityBufferRequest {
                label: format!("blas[{i}].vertices"),
                size: u64::try_from(tris.len()).ok().and_then(|n| n.checked_mul(4))
                    .ok_or("AS capacity: vertex size overflow")?,
                usage: BUFFER_USAGE_SHADER_DEVICE_ADDRESS | BUFFER_USAGE_ACCEL_STRUCTURE_BUILD_INPUT_READ_ONLY
                    | BUFFER_USAGE_STORAGE_BUFFER
                    | if updatable_blas.contains(&(i as u32)) { BUFFER_USAGE_TRANSFER_DST } else { 0 },
                host_visible: true,
            });
        }
        requests.push(AsCapacityBufferRequest {
            label: "tlas.instances".into(),
            size: u64::from(instance_count).checked_mul(std::mem::size_of::<AccelInstance>() as u64)
                .ok_or("AS capacity: instance size overflow")?,
            usage: BUFFER_USAGE_SHADER_DEVICE_ADDRESS | BUFFER_USAGE_ACCEL_STRUCTURE_BUILD_INPUT_READ_ONLY,
            host_visible: true,
        });
        Ok(requests)
    }
}

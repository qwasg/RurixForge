// RFC-0054 staged private immutable texture placement evidence. Safe only.
// Does not change selection, allocation, mapping, synchronization or C ABI.
// Assisted-by: Codex:GPT-6. Included inside render_exec after existing items.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HqHostTexturePlacement {
    pub resource: u32,
    pub allocation: Option<StableAllocationId>,
    pub requested_bytes: u64,
    pub required_bytes: u64,
    pub alignment: u64,
    pub memory_type_bits: u32,
    pub memory_type_index: u32,
    pub memory_property_flags: u32,
    pub heap_index: u32,
    pub heap_flags: u32,
    /// Derived with the unchanged default picker and this same buffer's type bits;
    /// not another VkBuffer query and not a historical R21 measurement.
    pub default_device_local_type: Option<u32>,
    pub default_device_local_heap: Option<u32>,
}
impl HqHostTexturePlacement {
    pub fn require_non_device_host(&self)->Result<(),String> {
        if self.requested_bytes==0 || self.required_bytes<self.requested_bytes || self.alignment==0
            || self.memory_type_index>=32 || self.memory_type_bits&(1u32<<self.memory_type_index)==0
            || self.memory_property_flags&(MEM_HOST_VISIBLE|MEM_HOST_COHERENT)!=(MEM_HOST_VISIBLE|MEM_HOST_COHERENT)
            || self.memory_property_flags&MEM_DEVICE_LOCAL!=0 || self.heap_flags&1!=0
            || self.default_device_local_type.is_none() || self.default_device_local_heap.is_none()
            || self.default_device_local_heap==Some(self.heap_index) {
            return Err("immutable texture requires real coherent host type on a non-device-local heap".into());
        }
        Ok(())
    }
    pub fn same_requirement(&self,other:&Self)->bool {
        let mut a=self.clone();let mut b=other.clone();a.allocation=None;b.allocation=None;a==b
    }
    pub fn evidence_json(&self)->String {
        format!("{{\"resource\":{},\"allocation\":{},\"requested_bytes\":{},\"required_bytes\":{},\"alignment\":{},\"memory_type_bits\":{},\"memory_type_index\":{},\"memory_property_flags\":{},\"heap_index\":{},\"heap_flags\":{},\"default_device_local_type\":{},\"default_device_local_heap\":{}}}",
            self.resource,self.allocation.map_or("null".into(),|a|a.0.to_string()),self.requested_bytes,self.required_bytes,self.alignment,
            self.memory_type_bits,self.memory_type_index,self.memory_property_flags,self.heap_index,self.heap_flags,
            self.default_device_local_type.map_or("null".into(),|v|v.to_string()),self.default_device_local_heap.map_or("null".into(),|v|v.to_string()))
    }
}

impl FrameCapacityReport {
    /// Separate opt-in witness; legacy capacity JSON serialization is untouched.
    pub fn hq_host_texture_placements(&self,ids:&[u32])->Result<Vec<HqHostTexturePlacement>,String> {
        self.require_fit()?;
        let mut out=Vec::new();
        for (ordinal,&resource) in ids.iter().enumerate() {
            if ids[..ordinal].contains(&resource) {return Err("duplicate immutable texture resource".into());}
            let label=format!("resource[{resource}]");
            let rows=self.allocations.iter().filter(|a|a.label==label && a.phase=="persistent").collect::<Vec<_>>();
            if rows.len()!=1 || self.allocations.iter().any(|a|a.label==format!("{label}.upload")) {
                return Err("host texture persistent query is missing, duplicated or staged".into());
            }
            let a=rows[0];let heap_flags=*self.memory_heap_flags.get(a.heap_index as usize).ok_or("actual queried texture heap absent")?;
            let baseline=self.memory_type_properties.iter().enumerate().find(|(i,(flags,_))|
                a.memory_type_bits&(1u32<<i)!=0 && flags&MEM_DEVICE_LOCAL!=0);
            let row=HqHostTexturePlacement{resource,allocation:None,requested_bytes:a.requested_bytes,required_bytes:a.required_bytes,
                alignment:a.alignment,memory_type_bits:a.memory_type_bits,memory_type_index:a.memory_type_index,
                memory_property_flags:a.memory_property_flags,heap_index:a.heap_index,heap_flags,
                default_device_local_type:baseline.map(|(i,_)|i as u32),default_device_local_heap:baseline.map(|(_,(_,heap))|*heap)};
            row.require_non_device_host()?;
            let heap=self.heaps.iter().find(|h|h.heap_index==row.heap_index).ok_or("queried host budget heap absent")?;
            if !heap.fits || heap.budget_bytes==0 || heap.persistent_bytes<row.required_bytes {
                return Err("texture host heap lacks counted persistent capacity".into());
            }
            out.push(row);
        }
        Ok(out)
    }
}

impl<'a> DeviceFrameSession<'a> {
    /// Reads the allocation choices recorded at the actual create/map boundary.
    /// Imports/exports and non-buffer or non-initialized roles cannot obtain this witness.
    pub fn hq_host_texture_placements(&self,ids:&[u32])->Result<Vec<HqHostTexturePlacement>,String> {
        let expected=self.capacity_preflight().ok_or("texture placement lacks constructor query")?.hq_host_texture_placements(ids)?;
        let mut result=Vec::new();
        for e in expected {
            let id=e.resource;
            let Some(ResourceDesc::Buffer(desc))=self.resources.get(id as usize) else{return Err("texture is not a buffer".into());};
            if desc.device_local || desc.usage.indirect || !desc.usage.storage || desc.usage.uniform || desc.usage.vertex
                || desc.data.is_none_or(|data|data.len()as u64!=desc.size)
                || self.native.frame.imported_indices.contains(&id)
                || self.native.frame.exportable_meta.iter().any(|r|r.0==id)
                || self.native.frame.exportable_buf_sizes.iter().any(|r|r.0==id) {
                return Err("immutable texture actual resource acquired a mutable/external role".into());
            }
            let rows=self.native.frame.cleanup.allocations.iter()
                .filter(|a|a.entry.resource_id==Some(StableResourceId(u64::from(id)+1))).collect::<Vec<_>>();
            if rows.len()!=1 {return Err("immutable texture lacks one actual allocation".into());}
            let actual=rows[0];let choice=actual.host_texture_choice.as_ref().ok_or("actual allocation has no host buffer choice witness")?;
            let Some(RtRes::Buf(buffer))=self.native.frame.rt.get(id as usize) else{return Err("actual texture buffer missing".into());};
            if buffer.mem!=actual.memory || buffer.size!=desc.size || choice.allocation!=Some(actual.entry.allocation_id)
                || choice.required_bytes!=actual.entry.bytes || choice.heap_index!=actual.entry.heap_index || !choice.same_requirement(&e) {
                return Err("actual texture allocation/type/requirements differ from constructor query".into());
            }
            choice.require_non_device_host()?;
            result.push(choice.clone());
        }
        Ok(result)
    }
}

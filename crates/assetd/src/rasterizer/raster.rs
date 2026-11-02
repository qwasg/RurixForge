//! Pure CPU 4-panel software rasterizer with z-buffer and directional lighting.
//! Renders 1 perspective showcase view (isometric 3/4) and 3 orthographic views (front, side, top)
//! into a composite 2x2 grid image, complete with grid dividing lines and embedded 5x7 ASCII text labels.

use base64::Engine as _;
use image::{Rgba, RgbaImage};

use super::extract::MeshGeometry;
use super::font::render_text;

/// Rasterizer configuration options.
#[derive(Debug, Clone)]
pub struct RasterOptions {
    /// Quadrant panel size in pixels (total sheet is 2*panel_size x 2*panel_size). Default 256.
    pub panel_size: u32,
    /// Background RGBA color. Default: [24, 26, 32, 255] (dark slate).
    pub background: [u8; 4],
    /// Directional light vector in camera space. Default: normalized [1, 1, 1].
    pub light_dir: [f32; 3],
    /// Ambient lighting floor intensity (0.0 to 1.0). Default: 0.25.
    pub ambient: f32,
    /// Directional contrast scale. Default: 0.75.
    pub diffuse_scale: f32,
    /// Whether to render quadrant divider lines. Default: true.
    pub draw_grid: bool,
    /// Whether to render quadrant text labels. Default: true.
    pub draw_labels: bool,
    /// Whether to draw wireframe overlay on triangles. Default: false.
    pub wireframe: bool,
}

impl Default for RasterOptions {
    fn default() -> Self {
        Self {
            panel_size: 256,
            background: [24, 26, 32, 255],
            light_dir: [0.57735, 0.57735, 0.57735],
            ambient: 0.25,
            diffuse_scale: 0.75,
            draw_grid: true,
            draw_labels: true,
            wireframe: false,
        }
    }
}

/// Pure CPU 4-Panel Software Rasterizer.
pub struct PureCpuRasterizer {
    pub options: RasterOptions,
}

impl PureCpuRasterizer {
    pub fn new(panel_size: u32) -> Self {
        Self {
            options: RasterOptions {
                panel_size,
                ..Default::default()
            },
        }
    }

    pub fn with_options(options: RasterOptions) -> Self {
        Self { options }
    }

    /// Render a `MeshGeometry` into a 4-panel composite RGBA image.
    pub fn render(&self, mesh: &MeshGeometry) -> RgbaImage {
        let s = self.options.panel_size;
        let total_w = s * 2;
        let total_h = s * 2;

        let mut image = RgbaImage::from_pixel(total_w, total_h, Rgba(self.options.background));

        // Framing & centering
        let center = mesh.bounding_box.center();
        let radius = mesh.bounding_box.radius();
        let scale = (s as f32 * 0.35) / radius;

        // Centered vertices: v' = v - center
        let centered_positions: Vec<[f32; 3]> = mesh
            .positions
            .iter()
            .map(|p| [p[0] - center[0], p[1] - center[1], p[2] - center[2]])
            .collect();

        // 1. Quadrant 1 (Top-Left): Perspective Showcase (azimuth 45°, elevation 30°)
        self.render_quadrant_perspective(
            &mut image,
            &centered_positions,
            mesh,
            scale,
            0,
            0,
            [100, 160, 220],
        );

        // 2. Quadrant 2 (Top-Right): Front Orthographic (X-Y, looking along -Z)
        self.render_quadrant_front(
            &mut image,
            &centered_positions,
            mesh,
            scale,
            s,
            0,
            [220, 120, 100],
        );

        // 3. Quadrant 3 (Bottom-Left): Side Orthographic (Z-Y, looking along -X)
        self.render_quadrant_side(
            &mut image,
            &centered_positions,
            mesh,
            scale,
            0,
            s,
            [120, 220, 100],
        );

        // 4. Quadrant 4 (Bottom-Right): Top Orthographic (X-Z, looking along -Y)
        self.render_quadrant_top(
            &mut image,
            &centered_positions,
            mesh,
            scale,
            s,
            s,
            [220, 220, 100],
        );

        // Draw dividing grid lines
        if self.options.draw_grid {
            let grid_color = [60, 64, 76, 255];
            // Vertical dividing line
            for y in 0..total_h {
                image.put_pixel(s, y, Rgba(grid_color));
            }
            // Horizontal dividing line
            for x in 0..total_w {
                image.put_pixel(x, s, Rgba(grid_color));
            }
        }

        // Draw embedded 5x7 ASCII text labels
        if self.options.draw_labels {
            let label_color = [200, 200, 200, 255];
            render_text(&mut image, 8, 8, "Perspective", label_color);
            render_text(&mut image, s + 8, 8, "Front (X-Y)", label_color);
            render_text(&mut image, 8, s + 8, "Side (Z-Y)", label_color);
            render_text(&mut image, s + 8, s + 8, "Top (X-Z)", label_color);
        }

        image
    }

    /// Render synthetic cube mesh matching reference E2E implementation.
    pub fn render_cube_mesh(
        &self,
        dimensions: (f32, f32, f32),
    ) -> (RgbaImage, serde_json::Value) {
        let mesh = MeshGeometry::cube([dimensions.0, dimensions.1, dimensions.2], [100, 160, 220]);
        let sheet = self.render(&mesh);

        let metadata = serde_json::json!({
            "dimensions": {
                "x": dimensions.0,
                "y": dimensions.1,
                "z": dimensions.2
            },
            "boundingBox": {
                "min": mesh.bounding_box.min,
                "max": mesh.bounding_box.max
            },
            "polycount": mesh.indices.len() / 3,
            "materials": mesh.materials
        });

        (sheet, metadata)
    }

    /// Export an RGBA image as a base64-encoded PNG data URL.
    pub fn export_data_url(&self, image: &RgbaImage) -> String {
        let mut buf = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut buf, image::ImageFormat::Png)
            .expect("PNG encode failed");
        let b64 = base64::engine::general_purpose::STANDARD.encode(buf.into_inner());
        format!("data:image/png;base64,{b64}")
    }

    // -----------------------------------------------------------------------
    // Quadrant Renderers
    // -----------------------------------------------------------------------

    fn render_quadrant_perspective(
        &self,
        image: &mut RgbaImage,
        centered_pos: &[[f32; 3]],
        mesh: &MeshGeometry,
        scale: f32,
        offset_x: u32,
        offset_y: u32,
        tint: [u8; 3],
    ) {
        let s = self.options.panel_size;
        let cx = offset_x as f32 + (s as f32 * 0.5);
        let cy = offset_y as f32 + (s as f32 * 0.5);

        // Isometric Yaw 45 deg, Pitch 30 deg rotation matrix:
        // cos(45°) = sqrt(2)/2 ≈ 0.70710678, sin(45°) ≈ 0.70710678
        // cos(30°) = sqrt(3)/2 ≈ 0.8660254,  sin(30°) = 0.5
        let cos_y = 0.70710678f32;
        let sin_y = 0.70710678f32;
        let cos_p = 0.8660254f32;
        let sin_p = 0.5f32;

        let project_pt = |p: [f32; 3]| -> ([f32; 3], f32) {
            let px = cos_y * p[0] - sin_y * p[2];
            let py = sin_y * sin_p * p[0] + cos_p * p[1] + cos_y * sin_p * p[2];
            let pz = sin_y * cos_p * p[0] - sin_p * p[1] + cos_y * cos_p * p[2];
            let sx = cx + px * scale;
            let sy = cy - py * scale;
            ([sx, sy, pz], pz)
        };

        let rotate_norm = |n: [f32; 3]| -> [f32; 3] {
            let nx = cos_y * n[0] - sin_y * n[2];
            let ny = sin_y * sin_p * n[0] + cos_p * n[1] + cos_y * sin_p * n[2];
            let nz = sin_y * cos_p * n[0] - sin_p * n[1] + cos_y * cos_p * n[2];
            [nx, ny, nz]
        };

        self.rasterize_quadrant(
            image,
            centered_pos,
            mesh,
            &project_pt,
            &rotate_norm,
            offset_x,
            offset_y,
            tint,
        );
    }

    fn render_quadrant_front(
        &self,
        image: &mut RgbaImage,
        centered_pos: &[[f32; 3]],
        mesh: &MeshGeometry,
        scale: f32,
        offset_x: u32,
        offset_y: u32,
        tint: [u8; 3],
    ) {
        let s = self.options.panel_size;
        let cx = offset_x as f32 + (s as f32 * 0.5);
        let cy = offset_y as f32 + (s as f32 * 0.5);

        // Front: look along -Z (X is right, Y is up)
        let project_pt = |p: [f32; 3]| -> ([f32; 3], f32) {
            let sx = cx + p[0] * scale;
            let sy = cy - p[1] * scale;
            ([sx, sy, p[2]], p[2])
        };

        let rotate_norm = |n: [f32; 3]| -> [f32; 3] { n };

        self.rasterize_quadrant(
            image,
            centered_pos,
            mesh,
            &project_pt,
            &rotate_norm,
            offset_x,
            offset_y,
            tint,
        );
    }

    fn render_quadrant_side(
        &self,
        image: &mut RgbaImage,
        centered_pos: &[[f32; 3]],
        mesh: &MeshGeometry,
        scale: f32,
        offset_x: u32,
        offset_y: u32,
        tint: [u8; 3],
    ) {
        let s = self.options.panel_size;
        let cx = offset_x as f32 + (s as f32 * 0.5);
        let cy = offset_y as f32 + (s as f32 * 0.5);

        // Side: look along -X (Z is right, Y is up)
        let project_pt = |p: [f32; 3]| -> ([f32; 3], f32) {
            let sx = cx + p[2] * scale;
            let sy = cy - p[1] * scale;
            ([sx, sy, p[0]], p[0])
        };

        let rotate_norm = |n: [f32; 3]| -> [f32; 3] { [n[2], n[1], n[0]] };

        self.rasterize_quadrant(
            image,
            centered_pos,
            mesh,
            &project_pt,
            &rotate_norm,
            offset_x,
            offset_y,
            tint,
        );
    }

    fn render_quadrant_top(
        &self,
        image: &mut RgbaImage,
        centered_pos: &[[f32; 3]],
        mesh: &MeshGeometry,
        scale: f32,
        offset_x: u32,
        offset_y: u32,
        tint: [u8; 3],
    ) {
        let s = self.options.panel_size;
        let cx = offset_x as f32 + (s as f32 * 0.5);
        let cy = offset_y as f32 + (s as f32 * 0.5);

        // Top: look along -Y (X is right, Z is down)
        let project_pt = |p: [f32; 3]| -> ([f32; 3], f32) {
            let sx = cx + p[0] * scale;
            let sy = cy + p[2] * scale;
            ([sx, sy, p[1]], p[1])
        };

        let rotate_norm = |n: [f32; 3]| -> [f32; 3] { [n[0], -n[2], n[1]] };

        self.rasterize_quadrant(
            image,
            centered_pos,
            mesh,
            &project_pt,
            &rotate_norm,
            offset_x,
            offset_y,
            tint,
        );
    }

    // -----------------------------------------------------------------------
    // Core Rasterization & Shading Loop
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn rasterize_quadrant<FProj, FNorm>(
        &self,
        image: &mut RgbaImage,
        centered_pos: &[[f32; 3]],
        mesh: &MeshGeometry,
        project_pt: &FProj,
        rotate_norm: &FNorm,
        offset_x: u32,
        offset_y: u32,
        tint: [u8; 3],
    ) where
        FProj: Fn([f32; 3]) -> ([f32; 3], f32),
        FNorm: Fn([f32; 3]) -> [f32; 3],
    {
        let s = self.options.panel_size as usize;
        let mut z_buffer = vec![f32::NEG_INFINITY; s * s];

        // Project all vertices once for this quadrant
        let mut screen_pts: Vec<[f32; 3]> = Vec::with_capacity(centered_pos.len());
        for p in centered_pos {
            let (proj, _) = project_pt(*p);
            screen_pts.push(proj);
        }

        // Project all normals once for this quadrant
        let mut rot_normals: Vec<[f32; 3]> = Vec::with_capacity(mesh.normals.len());
        for n in &mesh.normals {
            rot_normals.push(rotate_norm(*n));
        }

        let min_qx = offset_x as f32;
        let max_qx = (offset_x + self.options.panel_size - 1) as f32;
        let min_qy = offset_y as f32;
        let max_qy = (offset_y + self.options.panel_size - 1) as f32;

        let light = self.options.light_dir;
        let ambient = self.options.ambient;
        let diffuse = self.options.diffuse_scale;

        // Triangle Rasterization
        for tri in mesh.indices.chunks_exact(3) {
            let i0 = tri[0] as usize;
            let i1 = tri[1] as usize;
            let i2 = tri[2] as usize;
            if i0 >= screen_pts.len() || i1 >= screen_pts.len() || i2 >= screen_pts.len() {
                continue;
            }

            let p0 = screen_pts[i0];
            let p1 = screen_pts[i1];
            let p2 = screen_pts[i2];

            let n0 = rot_normals.get(i0).copied().unwrap_or([0.0, 1.0, 0.0]);
            let n1 = rot_normals.get(i1).copied().unwrap_or([0.0, 1.0, 0.0]);
            let n2 = rot_normals.get(i2).copied().unwrap_or([0.0, 1.0, 0.0]);

            // Double signed area: 2A
            let area2 = (p1[0] - p0[0]) * (p2[1] - p0[1]) - (p1[1] - p0[1]) * (p2[0] - p0[0]);
            if area2.abs() < 1e-6 {
                continue;
            }

            // Screen bounding box clamped to this quadrant's pixel coordinates
            let tri_min_x = p0[0].min(p1[0]).min(p2[0]).max(min_qx);
            let tri_max_x = p0[0].max(p1[0]).max(p2[0]).min(max_qx);
            let tri_min_y = p0[1].min(p1[1]).min(p2[1]).max(min_qy);
            let tri_max_y = p0[1].max(p1[1]).max(p2[1]).min(max_qy);

            let x_start = tri_min_x.floor() as i32;
            let x_end = tri_max_x.ceil() as i32;
            let y_start = tri_min_y.floor() as i32;
            let y_end = tri_max_y.ceil() as i32;

            if x_start > x_end || y_start > y_end {
                continue;
            }

            let inv_area2 = 1.0 / area2;

            for y in y_start..=x_end.max(y_end) {
                if y < offset_y as i32 || y >= (offset_y + self.options.panel_size) as i32 {
                    continue;
                }
                let py = y as f32 + 0.5;
                let local_y = (y - offset_y as i32) as usize;

                for x in x_start..=x_end {
                    if x < offset_x as i32 || x >= (offset_x + self.options.panel_size) as i32 {
                        continue;
                    }
                    let px = x as f32 + 0.5;
                    let local_x = (x - offset_x as i32) as usize;

                    // Barycentric coordinates
                    let w0 = ((p1[0] - px) * (p2[1] - py) - (p1[1] - py) * (p2[0] - px)) * inv_area2;
                    let w1 = ((p2[0] - px) * (p0[1] - py) - (p2[1] - py) * (p0[0] - px)) * inv_area2;
                    let w2 = 1.0 - w0 - w1;

                    // Two-sided test: allow both clockwise and counter-clockwise winding
                    if (w0 >= -1e-5 && w1 >= -1e-5 && w2 >= -1e-5)
                        || (w0 <= 1e-5 && w1 <= 1e-5 && w2 <= 1e-5)
                    {
                        let z = w0 * p0[2] + w1 * p1[2] + w2 * p2[2];
                        let z_idx = local_y * s + local_x;

                        if z > z_buffer[z_idx] {
                            z_buffer[z_idx] = z;

                            // Interpolated normal
                            let nx = w0 * n0[0] + w1 * n1[0] + w2 * n2[0];
                            let ny = w0 * n0[1] + w1 * n1[1] + w2 * n2[1];
                            let nz = w0 * n0[2] + w1 * n1[2] + w2 * n2[2];
                            let n_len = (nx * nx + ny * ny + nz * nz).sqrt();
                            let norm = if n_len > 1e-6 {
                                [nx / n_len, ny / n_len, nz / n_len]
                            } else {
                                [0.0, 1.0, 0.0]
                            };

                            // Two-sided directional lighting
                            let dot = (norm[0] * light[0] + norm[1] * light[1] + norm[2] * light[2])
                                .abs();
                            let intensity = (ambient + diffuse * dot).clamp(0.0, 1.0);

                            let r = (tint[0] as f32 * intensity).clamp(0.0, 255.0) as u8;
                            let g = (tint[1] as f32 * intensity).clamp(0.0, 255.0) as u8;
                            let b = (tint[2] as f32 * intensity).clamp(0.0, 255.0) as u8;

                            image.put_pixel(x as u32, y as u32, Rgba([r, g, b, 255]));
                        }
                    }
                }
            }
        }

        // Draw explicit wireframe edges if present (such as in cube meshes or wireframe mode)
        if !mesh.edges.is_empty() || self.options.wireframe {
            let edge_color = [tint[0], tint[1], tint[2], 255];
            let clip_min_x = offset_x as i32;
            let clip_max_x = (offset_x + self.options.panel_size - 1) as i32;
            let clip_min_y = offset_y as i32;
            let clip_max_y = (offset_y + self.options.panel_size - 1) as i32;

            for &(i1, i2) in &mesh.edges {
                if i1 < screen_pts.len() && i2 < screen_pts.len() {
                    let p1 = screen_pts[i1];
                    let p2 = screen_pts[i2];
                    draw_line(
                        image,
                        p1[0].round() as i32,
                        p1[1].round() as i32,
                        p2[0].round() as i32,
                        p2[1].round() as i32,
                        edge_color,
                        2,
                        clip_min_x,
                        clip_min_y,
                        clip_max_x,
                        clip_max_y,
                    );
                }
            }
        }
    }
}

/// Bresenham's line rasterization with thickness and rectangular boundary clipping.
#[allow(clippy::too_many_arguments)]
pub fn draw_line(
    image: &mut RgbaImage,
    mut x0: i32,
    mut y0: i32,
    x1: i32,
    y1: i32,
    color: [u8; 4],
    width: i32,
    clip_min_x: i32,
    clip_min_y: i32,
    clip_max_x: i32,
    clip_max_y: i32,
) {
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        // Draw pixel with width
        for wx in 0..width {
            for wy in 0..width {
                let px = x0 + wx;
                let py = y0 + wy;
                if px >= clip_min_x && px <= clip_max_x && py >= clip_min_y && py <= clip_max_y {
                    image.put_pixel(px as u32, py as u32, Rgba(color));
                }
            }
        }

        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

//! Pure CPU 4-panel software rasterizer and 3D geometry extraction module.
//!
//! Provides deterministic offscreen rasterization of 3D assets (.glb, .rxmodel, .rxmesh)
//! generating a 2x2 composite inspection sheet (perspective showcase + front, side, and top orthographic views)
//! with directional lighting, z-buffer occlusion, quadrant dividing grid lines, and embedded ASCII labels.

pub mod extract;
pub mod font;
pub mod raster;

pub use extract::{
    compute_normals, extract_from_glb_bytes, extract_from_model_bundle,
    extract_from_rxmesh_bytes, extract_from_rxmodel_bytes, extract_geometry_from_file,
    AABB, BoundingBox3D, MeshGeometry,
};
pub use font::{render_text, FONT_5X7};
pub use raster::{draw_line, PureCpuRasterizer, RasterOptions};

use image::RgbaImage;

/// High-level function to render a 4-panel showcase sheet from a `MeshGeometry`
/// and return the composite RGBA image alongside its base64 data URL.
pub fn render_mesh_to_sheet(mesh: &MeshGeometry, panel_size: u32) -> (RgbaImage, String) {
    let rasterizer = PureCpuRasterizer::new(panel_size);
    let image = rasterizer.render(mesh);
    let data_url = rasterizer.export_data_url(&image);
    (image, data_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cube_rasterizer_quadrants_non_empty() {
        let rast = PureCpuRasterizer::new(64);
        let (sheet, meta) = rast.render_cube_mesh((2.0, 2.0, 2.0));
        assert_eq!(sheet.width(), 128);
        assert_eq!(sheet.height(), 128);

        // Check that quadrant 1 (perspective) has non-background pixels
        let mut q1_colored = 0;
        for y in 0..64 {
            for x in 0..64 {
                let pixel = sheet.get_pixel(x, y);
                if pixel.0 != [24, 26, 32, 255] {
                    q1_colored += 1;
                }
            }
        }
        assert!(q1_colored > 20, "Q1 should contain rendered pixels");

        // Check other quadrants
        for (qx, qy, name) in [(64, 0, "Front"), (0, 64, "Side"), (64, 64, "Top")] {
            let mut colored = 0;
            for y in qy..qy + 64 {
                for x in qx..qx + 64 {
                    let pixel = sheet.get_pixel(x, y);
                    if pixel.0 != [24, 26, 32, 255] {
                        colored += 1;
                    }
                }
            }
            assert!(colored > 20, "{name} quadrant should contain rendered pixels");
        }

        assert_eq!(meta["polycount"], 12);
    }

    #[test]
    fn test_zero_dimension_mesh_safe() {
        let rast = PureCpuRasterizer::new(64);
        let (sheet, meta) = rast.render_cube_mesh((0.0, 0.0, 0.0));
        assert_eq!(sheet.width(), 128);
        assert_eq!(sheet.height(), 128);
        assert_eq!(meta["dimensions"]["x"], 0.0);
    }

    #[test]
    fn test_extreme_aspect_ratio_needle() {
        let rast = PureCpuRasterizer::new(64);
        let (sheet, meta) = rast.render_cube_mesh((100.0, 0.1, 0.1));
        assert_eq!(sheet.width(), 128);
        assert_eq!(sheet.height(), 128);
        assert_eq!(meta["dimensions"]["x"], 100.0);
    }

    #[test]
    fn test_export_data_url_valid() {
        let rast = PureCpuRasterizer::new(64);
        let (sheet, _) = rast.render_cube_mesh((1.0, 1.0, 1.0));
        let url = rast.export_data_url(&sheet);
        assert!(url.starts_with("data:image/png;base64,"));
        assert!(url.len() > 100);
    }

    #[test]
    fn test_accurate_bounding_box() {
        let rast = PureCpuRasterizer::new(64);
        let (_, meta) = rast.render_cube_mesh((4.0, 2.5, 6.0));
        assert_eq!(meta["dimensions"]["x"], 4.0);
        assert_eq!(meta["dimensions"]["y"], 2.5);
        assert_eq!(meta["dimensions"]["z"], 6.0);
    }
}

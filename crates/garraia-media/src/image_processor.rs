//! Image processing module for GarraIA
//!
//! Provides image analysis, metadata extraction, and format conversion.
//! Note: Full OCR requires external service integration or tesseract.

use garraia_common::{Error, Result};
use image::{GenericImageView, ImageFormat};
use std::path::Path;
use tracing::{debug, info};

use crate::types::ImageAnalysis;

/// Processor for images
pub struct ImageProcessor;

impl ImageProcessor {
    /// Create a new image processor
    pub fn new() -> Self {
        Self
    }

    /// Analyze an image file and extract metadata
    ///
    /// # Arguments
    /// * `path` - Path to the image file
    ///
    /// # Returns
    /// * `ImageAnalysis` containing image metadata
    pub fn analyze<P: AsRef<Path>>(&self, path: P) -> Result<ImageAnalysis> {
        let path = path.as_ref();
        info!("Analyzing image: {}", path.display());

        let img =
            image::open(path).map_err(|e| Error::Media(format!("Failed to open image: {}", e)))?;

        let (width, height) = img.dimensions();
        let format = self.detect_format(path);

        debug!("Image dimensions: {}x{}, format: {}", width, height, format);

        Ok(ImageAnalysis {
            text: None,        // OCR requires external service
            description: None, // Description requires AI service
            width,
            height,
            format,
        })
    }

    /// Analyze image from bytes
    ///
    /// # Arguments
    /// * `data` - Image file bytes
    ///
    /// # Returns
    /// * `ImageAnalysis` containing image metadata
    pub fn analyze_from_bytes(&self, data: &[u8]) -> Result<ImageAnalysis> {
        info!("Analyzing image from bytes ({} bytes)", data.len());

        let img = image::load_from_memory(data)
            .map_err(|e| Error::Media(format!("Failed to load image from bytes: {}", e)))?;

        let (width, height) = img.dimensions();

        // Try to detect format from magic bytes
        let format = self.detect_format_from_bytes(data);

        debug!("Image dimensions: {}x{}, format: {}", width, height, format);

        Ok(ImageAnalysis {
            text: None,
            description: None,
            width,
            height,
            format,
        })
    }

    /// Get image dimensions without full analysis
    pub fn get_dimensions<P: AsRef<Path>>(&self, path: P) -> Result<(u32, u32)> {
        let img = image::open(path.as_ref())
            .map_err(|e| Error::Media(format!("Failed to open image: {}", e)))?;
        Ok(img.dimensions())
    }

    /// Resize an image to fit within max dimensions while maintaining aspect ratio
    ///
    /// # Arguments
    /// * `path` - Path to the input image
    /// * `output_path` - Path for the output image
    /// * `max_width` - Maximum width
    /// * `max_height` - Maximum height
    pub fn resize<P: AsRef<Path>>(
        &self,
        path: P,
        output_path: P,
        max_width: u32,
        max_height: u32,
    ) -> Result<()> {
        let input_path = path.as_ref();
        let output_path = output_path.as_ref();

        info!(
            "Resizing image {} to max {}x{}",
            input_path.display(),
            max_width,
            max_height
        );

        let img = image::open(input_path)
            .map_err(|e| Error::Media(format!("Failed to open image: {}", e)))?;

        let resized = img.resize(max_width, max_height, image::imageops::FilterType::Lanczos3);

        resized
            .save(output_path)
            .map_err(|e| Error::Media(format!("Failed to save resized image: {}", e)))?;

        Ok(())
    }

    /// Convert image to a different format
    ///
    /// # Arguments
    /// * `input_path` - Path to the input image
    /// * `output_path` - Path for the output image
    /// * `format` - Target format (png, jpg, webp, etc.)
    pub fn convert<P: AsRef<Path>>(
        &self,
        input_path: P,
        output_path: P,
        format: &str,
    ) -> Result<()> {
        let input_path = input_path.as_ref();
        let output_path = output_path.as_ref();

        info!("Converting image {} to {}", input_path.display(), format);

        let img = image::open(input_path)
            .map_err(|e| Error::Media(format!("Failed to open image: {}", e)))?;

        let output_format = match format.to_lowercase().as_str() {
            "png" => ImageFormat::Png,
            "jpg" | "jpeg" => ImageFormat::Jpeg,
            "webp" => ImageFormat::WebP,
            "gif" => ImageFormat::Gif,
            "bmp" => ImageFormat::Bmp,
            "ico" => ImageFormat::Ico,
            _ => {
                return Err(Error::Media(format!(
                    "Unsupported output format: {}",
                    format
                )));
            }
        };

        img.save_with_format(output_path, output_format)
            .map_err(|e| Error::Media(format!("Failed to save image: {}", e)))?;

        Ok(())
    }

    /// Generate thumbnail for an image
    ///
    /// # Arguments
    /// * `input_path` - Path to the input image
    /// * `output_path` - Path for the thumbnail
    /// * `size` - Maximum dimension for the thumbnail
    pub fn thumbnail<P: AsRef<Path>>(
        &self,
        input_path: P,
        output_path: P,
        size: u32,
    ) -> Result<()> {
        let input_path = input_path.as_ref();
        let output_path = output_path.as_ref();

        info!(
            "Creating thumbnail for {} (size: {})",
            input_path.display(),
            size
        );

        let img = image::open(input_path)
            .map_err(|e| Error::Media(format!("Failed to open image: {}", e)))?;

        let thumbnail = img.thumbnail(size, size);

        thumbnail
            .save(output_path)
            .map_err(|e| Error::Media(format!("Failed to save thumbnail: {}", e)))?;

        Ok(())
    }

    /// Create a grayscale version of an image
    pub fn grayscale<P: AsRef<Path>>(&self, input_path: P, output_path: P) -> Result<()> {
        let input_path = input_path.as_ref();
        let output_path = output_path.as_ref();

        let img = image::open(input_path)
            .map_err(|e| Error::Media(format!("Failed to open image: {}", e)))?;

        let grayscale = img.grayscale();

        grayscale
            .save(output_path)
            .map_err(|e| Error::Media(format!("Failed to save grayscale image: {}", e)))?;

        Ok(())
    }

    /// Get basic image statistics
    pub fn get_stats<P: AsRef<Path>>(&self, path: P) -> Result<ImageStats> {
        let img = image::open(path.as_ref())
            .map_err(|e| Error::Media(format!("Failed to open image: {}", e)))?;

        let rgba = img.to_rgba8();
        let pixels = rgba.pixels();

        let mut r_sum: u64 = 0;
        let mut g_sum: u64 = 0;
        let mut b_sum: u64 = 0;
        let mut count: u64 = 0;

        for pixel in pixels {
            r_sum += pixel[0] as u64;
            g_sum += pixel[1] as u64;
            b_sum += pixel[2] as u64;
            count += 1;
        }

        Ok(ImageStats {
            mean_r: r_sum as f64 / count as f64,
            mean_g: g_sum as f64 / count as f64,
            mean_b: b_sum as f64 / count as f64,
            total_pixels: count,
        })
    }

    /// Detect format from file extension
    fn detect_format<P: AsRef<Path>>(&self, path: P) -> String {
        let path = path.as_ref();
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_else(|| "unknown".to_string())
    }

    /// Detect format from magic bytes
    fn detect_format_from_bytes(&self, data: &[u8]) -> String {
        if data.len() < 12 {
            return "unknown".to_string();
        }

        // Check magic bytes
        if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
            return "png".to_string();
        }
        if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return "jpeg".to_string();
        }
        if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
            return "gif".to_string();
        }
        if data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
            return "webp".to_string();
        }
        if data.starts_with(b"BM") {
            return "bmp".to_string();
        }

        "unknown".to_string()
    }
}

impl Default for ImageProcessor {
    fn default() -> Self {
        Self::new()
    }
}

/// Basic image statistics
#[derive(Debug, Clone)]
pub struct ImageStats {
    pub mean_r: f64,
    pub mean_g: f64,
    pub mean_b: f64,
    pub total_pixels: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::DynamicImage;
    use tempfile::TempDir;

    fn create_test_png(tmp_dir: &TempDir) -> std::path::PathBuf {
        let png_path = tmp_dir.path().join("test.png");

        // Create a simple 10x10 red PNG
        let img = DynamicImage::ImageRgba8(image::RgbaImage::from_fn(10, 10, |_x, _y| {
            image::Rgba([255, 0, 0, 255])
        }));

        img.save(&png_path).unwrap();
        png_path
    }

    #[test]
    fn test_image_processor_new() {
        let processor = ImageProcessor::new();
        let _ = processor;
    }

    #[test]
    fn test_analyze_png() {
        let tmp_dir = TempDir::new().unwrap();
        let png_path = create_test_png(&tmp_dir);

        let processor = ImageProcessor::new();
        let result = processor.analyze(&png_path);

        assert!(result.is_ok());
        let analysis = result.unwrap();
        assert_eq!(analysis.width, 10);
        assert_eq!(analysis.height, 10);
        assert_eq!(analysis.format, "png");
    }

    #[test]
    fn test_analyze_from_bytes() {
        let tmp_dir = TempDir::new().unwrap();
        let png_path = create_test_png(&tmp_dir);

        let bytes = std::fs::read(&png_path).unwrap();

        let processor = ImageProcessor::new();
        let result = processor.analyze_from_bytes(&bytes);

        assert!(result.is_ok());
        let analysis = result.unwrap();
        assert_eq!(analysis.width, 10);
        assert_eq!(analysis.height, 10);
    }

    #[test]
    fn test_get_dimensions() {
        let tmp_dir = TempDir::new().unwrap();
        let png_path = create_test_png(&tmp_dir);

        let processor = ImageProcessor::new();
        let result = processor.get_dimensions(&png_path);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), (10, 10));
    }

    #[test]
    fn test_resize() {
        let tmp_dir = TempDir::new().unwrap();
        let input_path = create_test_png(&tmp_dir);
        let output_path = tmp_dir.path().join("resized.png");

        let processor = ImageProcessor::new();
        let result = processor.resize(&input_path, &output_path, 5, 5);

        assert!(result.is_ok());
        assert!(output_path.exists());

        // Check resized dimensions
        let resized = image::open(&output_path).unwrap();
        assert!(resized.width() <= 5);
        assert!(resized.height() <= 5);
    }

    #[test]
    fn test_convert_format() {
        let tmp_dir = TempDir::new().unwrap();
        let input_path = create_test_png(&tmp_dir);
        let output_path = tmp_dir.path().join("converted.jpg");

        let processor = ImageProcessor::new();
        let result = processor.convert(&input_path, &output_path, "jpg");

        assert!(result.is_ok());
        assert!(output_path.exists());
    }

    #[test]
    fn test_thumbnail() {
        let tmp_dir = TempDir::new().unwrap();
        let input_path = create_test_png(&tmp_dir);
        let output_path = tmp_dir.path().join("thumb.png");

        let processor = ImageProcessor::new();
        let result = processor.thumbnail(&input_path, &output_path, 5);

        assert!(result.is_ok());
        assert!(output_path.exists());

        let thumb = image::open(&output_path).unwrap();
        assert!(thumb.width() <= 5);
        assert!(thumb.height() <= 5);
    }

    #[test]
    fn test_grayscale() {
        let tmp_dir = TempDir::new().unwrap();
        let input_path = create_test_png(&tmp_dir);
        let output_path = tmp_dir.path().join("gray.png");

        let processor = ImageProcessor::new();
        let result = processor.grayscale(&input_path, &output_path);

        assert!(result.is_ok());
        assert!(output_path.exists());
    }

    #[test]
    fn test_get_stats() {
        let tmp_dir = TempDir::new().unwrap();
        let png_path = create_test_png(&tmp_dir);

        let processor = ImageProcessor::new();
        let result = processor.get_stats(&png_path);

        assert!(result.is_ok());
        let stats = result.unwrap();
        // Red image should have high R mean, low G and B
        assert!(stats.mean_r > 200.0);
        assert!(stats.mean_g < 50.0);
        assert!(stats.mean_b < 50.0);
    }

    #[test]
    fn test_detect_format_from_bytes() {
        let processor = ImageProcessor::new();

        // PNG magic bytes
        let png_bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        assert_eq!(processor.detect_format_from_bytes(&png_bytes), "png");

        // JPEG magic bytes
        let jpeg_bytes = vec![0xFF, 0xD8, 0xFF, 0xE0];
        assert_eq!(processor.detect_format_from_bytes(&jpeg_bytes), "jpeg");

        // Unknown
        let unknown_bytes = vec![0x00, 0x00, 0x00, 0x00];
        assert_eq!(
            processor.detect_format_from_bytes(&unknown_bytes),
            "unknown"
        );
    }
}

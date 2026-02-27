use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MediaType {
    Image,
    Audio,
    Video,
    Document,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MediaFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
    Mp3,
    Ogg,
    Wav,
    Mp4,
    Webm,
    Pdf,
    Doc,
    Docx,
    Text,
    Other(String),
}

impl MediaFormat {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "png" => Self::Png,
            "jpg" | "jpeg" => Self::Jpeg,
            "gif" => Self::Gif,
            "webp" => Self::Webp,
            "mp3" => Self::Mp3,
            "ogg" => Self::Ogg,
            "wav" => Self::Wav,
            "mp4" => Self::Mp4,
            "webm" => Self::Webm,
            "pdf" => Self::Pdf,
            "doc" => Self::Doc,
            "docx" => Self::Docx,
            "txt" | "md" | "json" | "xml" | "html" | "htm" => Self::Text,
            other => Self::Other(other.to_string()),
        }
    }

    pub fn mime_type(&self) -> &str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
            Self::Mp3 => "audio/mpeg",
            Self::Ogg => "audio/ogg",
            Self::Wav => "audio/wav",
            Self::Mp4 => "video/mp4",
            Self::Webm => "video/webm",
            Self::Pdf => "application/pdf",
            Self::Doc => "application/msword",
            Self::Docx => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            Self::Text => "text/plain",
            Self::Other(_) => "application/octet-stream",
        }
    }

    /// Returns true if this format is a supported document format
    pub fn is_document(&self) -> bool {
        matches!(
            self,
            Self::Pdf | Self::Doc | Self::Docx | Self::Text | Self::Other(_)
        )
    }

    /// Returns true if this format is a supported image format
    pub fn is_image(&self) -> bool {
        matches!(self, Self::Png | Self::Jpeg | Self::Gif | Self::Webp)
    }
}

/// Result of document parsing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedDocument {
    pub text: String,
    pub page_count: usize,
    pub metadata: DocumentMetadata,
}

/// Document metadata
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub creator: Option<String>,
    pub producer: Option<String>,
    pub creation_date: Option<String>,
    pub modification_date: Option<String>,
}

/// Result of image analysis (OCR/description)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageAnalysis {
    pub text: Option<String>,
    pub description: Option<String>,
    pub width: u32,
    pub height: u32,
    pub format: String,
}

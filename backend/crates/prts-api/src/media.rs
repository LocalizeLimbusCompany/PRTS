//! 项目媒体的校验与本地原子存储。

use std::io::Cursor;
use std::path::{Path, PathBuf};

use futures_util::future::BoxFuture;

pub const AVATAR_CONTENT_TYPE: &str = "image/webp";
pub const AVATAR_MAX_BYTES: usize = 512 * 1024;
pub const AVATAR_MAX_DIMENSION: u32 = 1024;
pub const AVATAR_MAX_PIXELS: u64 = 1_048_576;

/// 项目头像校验错误；路由统一映射为 400。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvatarValidationError {
    Empty,
    TooLarge,
    InvalidSignature,
    DecodeFailed,
    NotSquare,
    DimensionExceeded,
    PixelLimitExceeded,
}

impl AvatarValidationError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Empty => "avatar_empty",
            Self::TooLarge => "avatar_too_large",
            Self::InvalidSignature => "avatar_invalid_webp_signature",
            Self::DecodeFailed => "avatar_decode_failed",
            Self::NotSquare => "avatar_must_be_square",
            Self::DimensionExceeded => "avatar_dimension_exceeded",
            Self::PixelLimitExceeded => "avatar_pixel_limit_exceeded",
        }
    }
}

/// 独立验证声明 MIME 之外的真实 WebP 内容与资源上限。
pub fn validate_project_avatar(bytes: &[u8]) -> Result<(), AvatarValidationError> {
    if bytes.is_empty() {
        return Err(AvatarValidationError::Empty);
    }
    if bytes.len() > AVATAR_MAX_BYTES {
        return Err(AvatarValidationError::TooLarge);
    }
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return Err(AvatarValidationError::InvalidSignature);
    }
    let reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::WebP);
    let (width, height) = reader
        .into_dimensions()
        .map_err(|_| AvatarValidationError::DecodeFailed)?;
    if u64::from(width) * u64::from(height) > AVATAR_MAX_PIXELS {
        return Err(AvatarValidationError::PixelLimitExceeded);
    }
    if width > AVATAR_MAX_DIMENSION || height > AVATAR_MAX_DIMENSION {
        return Err(AvatarValidationError::DimensionExceeded);
    }
    if width != height {
        return Err(AvatarValidationError::NotSquare);
    }
    image::load_from_memory_with_format(bytes, image::ImageFormat::WebP)
        .map_err(|_| AvatarValidationError::DecodeFailed)?;
    Ok(())
}

/// 校验请求声明的媒体类型，防止只凭文件扩展名接受内容。
pub fn validate_avatar_content_type(
    content_type: Option<&str>,
) -> Result<(), AvatarValidationError> {
    if content_type.is_some_and(|value| value.eq_ignore_ascii_case(AVATAR_CONTENT_TYPE)) {
        Ok(())
    } else {
        Err(AvatarValidationError::InvalidSignature)
    }
}

/// 媒体存储抽象；业务路由只传服务端生成的相对 key。
pub trait MediaStore: Send + Sync {
    fn write_atomic<'a>(
        &'a self,
        key: &'a str,
        bytes: &'a [u8],
    ) -> BoxFuture<'a, std::io::Result<()>>;
    fn read<'a>(&'a self, key: &'a str) -> BoxFuture<'a, std::io::Result<Vec<u8>>>;
    fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, std::io::Result<()>>;
}

/// 持久卷上的本地媒体存储。
#[derive(Debug, Clone)]
pub struct LocalMediaStore {
    root: PathBuf,
}

impl LocalMediaStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path_for(&self, key: &str) -> std::io::Result<PathBuf> {
        let path = Path::new(key);
        if path.is_absolute()
            || path
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "media key must contain only normal relative path segments",
            ));
        }
        Ok(self.root.join(path))
    }

    /// 为同目录临时文件生成不可预测名称，使 rename 始终留在同一文件系统。
    fn temporary_path(parent: &Path, suffix: &str) -> PathBuf {
        parent.join(format!(
            ".avatar-{}.{suffix}",
            prts_auth::token::random_token(12).to_lowercase()
        ))
    }
}

impl MediaStore for LocalMediaStore {
    fn write_atomic<'a>(
        &'a self,
        key: &'a str,
        bytes: &'a [u8],
    ) -> BoxFuture<'a, std::io::Result<()>> {
        Box::pin(async move {
            let destination = self.path_for(key)?;
            let parent = destination.parent().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "media key has no parent")
            })?;
            tokio::fs::create_dir_all(parent).await?;
            let temporary = Self::temporary_path(parent, "tmp");
            let backup = Self::temporary_path(parent, "bak");
            tokio::fs::write(&temporary, bytes).await?;

            // Windows 不允许 rename 覆盖现有文件；先把旧文件移到同目录备份，再发布新文件。
            let had_previous = match tokio::fs::rename(&destination, &backup).await {
                Ok(()) => true,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(error) => {
                    let _ = tokio::fs::remove_file(&temporary).await;
                    return Err(error);
                }
            };
            if let Err(error) = tokio::fs::rename(&temporary, &destination).await {
                let _ = tokio::fs::remove_file(&temporary).await;
                if had_previous {
                    let _ = tokio::fs::rename(&backup, &destination).await;
                }
                return Err(error);
            }
            if had_previous {
                if let Err(error) = tokio::fs::remove_file(&backup).await {
                    tracing::warn!(%error, "failed to remove replaced avatar backup");
                }
            }
            Ok(())
        })
    }

    fn read<'a>(&'a self, key: &'a str) -> BoxFuture<'a, std::io::Result<Vec<u8>>> {
        Box::pin(async move { tokio::fs::read(self.path_for(key)?).await })
    }

    fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, std::io::Result<()>> {
        Box::pin(async move {
            match tokio::fs::remove_file(self.path_for(key)?).await {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            }
        })
    }
}

pub fn project_avatar_key(project_id: i64) -> String {
    format!("projects/{project_id}/avatar.webp")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_webp(width: u32, height: u32) -> Vec<u8> {
        let pixels = vec![0_u8; (width * height * 4) as usize];
        let mut bytes = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut bytes)
            .encode(&pixels, width, height, image::ExtendedColorType::Rgba8)
            .unwrap();
        bytes
    }

    #[test]
    fn rejects_spoofed_or_oversized_payloads_before_decode() {
        assert_eq!(
            validate_project_avatar(b"not a webp"),
            Err(AvatarValidationError::InvalidSignature)
        );
        assert_eq!(
            validate_project_avatar(&vec![0; AVATAR_MAX_BYTES + 1]),
            Err(AvatarValidationError::TooLarge)
        );
    }

    #[test]
    fn rejects_missing_or_spoofed_content_type() {
        assert!(validate_avatar_content_type(Some("image/webp")).is_ok());
        assert!(validate_avatar_content_type(Some("IMAGE/WEBP")).is_ok());
        assert_eq!(
            validate_avatar_content_type(Some("image/png")),
            Err(AvatarValidationError::InvalidSignature)
        );
        assert_eq!(
            validate_avatar_content_type(None),
            Err(AvatarValidationError::InvalidSignature)
        );
    }

    #[test]
    fn validates_decoding_shape_and_resource_ceilings() {
        let square = encode_webp(32, 32);
        assert!(validate_project_avatar(&square).is_ok());
        assert_eq!(
            validate_project_avatar(b"RIFF\0\0\0\0WEBPbroken"),
            Err(AvatarValidationError::DecodeFailed)
        );
        assert_eq!(
            validate_project_avatar(&encode_webp(32, 16)),
            Err(AvatarValidationError::NotSquare)
        );
        assert_eq!(
            validate_project_avatar(&encode_webp(1025, 1025)),
            Err(AvatarValidationError::PixelLimitExceeded)
        );
        assert_eq!(
            validate_project_avatar(&encode_webp(1025, 1)),
            Err(AvatarValidationError::DimensionExceeded)
        );
    }

    #[test]
    fn local_store_rejects_traversal_keys() {
        let store = LocalMediaStore::new("media");
        assert!(store.path_for("../avatar.webp").is_err());
        assert!(store.path_for("projects/7/avatar.webp").is_ok());
    }
}

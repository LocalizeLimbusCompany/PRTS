package handlers

import (
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"

	"prts-translation-system/internal/platform"
	"prts-translation-system/internal/runtime"
	"prts-translation-system/internal/store"
)

type loginRequest struct {
	Email    string `json:"email"`
	Password string `json:"password"`
}

type updatePreferencesRequest struct {
	PreferredLocale         string `json:"preferredLocale"`
	PreferredSourceLanguage string `json:"preferredSourceLanguage"`
}

type updateProfileRequest struct {
	DisplayName string `json:"displayName"`
	AvatarURL   string `json:"avatarUrl"`
}

func Login(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		var req loginRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}

		email := platform.NormalizeText(req.Email)
		password := req.Password
		if email == "" || password == "" {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "email 和 password 不能为空")
			return
		}

		token, user, err := dataStore.Login(r.Context(), email, password)
		if errors.Is(err, store.ErrNotFound) || (err != nil && err.Error() == "invalid credentials") {
			platform.WriteError(w, r, http.StatusUnauthorized, "unauthorized", "邮箱或密码错误")
			return
		}
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "登录失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{
			"token": token,
			"user":  user,
		})
	}
}

func Logout(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		token := r.Header.Get("Authorization")
		token = middlewareToken(token)
		if token != "" {
			_ = dataStore.Logout(r.Context(), token)
		}
		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{"ok": true})
	}
}

func Me() http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		user, ok := platform.AuthUserFromContext(r.Context())
		if !ok {
			platform.WriteError(w, r, http.StatusUnauthorized, "unauthorized", "请先登录")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, user)
	}
}

func UpdateMyPreferences(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		authUser, ok := platform.AuthUserFromContext(r.Context())
		if !ok {
			platform.WriteError(w, r, http.StatusUnauthorized, "unauthorized", "请先登录")
			return
		}

		var req updatePreferencesRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}

		preferredLocale := platform.NormalizeText(req.PreferredLocale)
		if preferredLocale == "" {
			preferredLocale = authUser.PreferredLocale
		}
		preferredSourceLanguage := platform.NormalizeIdentifier(req.PreferredSourceLanguage)

		user, err := dataStore.UpdateUserPreferences(r.Context(), authUser.ID, preferredLocale, preferredSourceLanguage)
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "更新用户偏好失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, user)
	}
}

func UpdateMyProfile(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		authUser, ok := platform.AuthUserFromContext(r.Context())
		if !ok {
			platform.WriteError(w, r, http.StatusUnauthorized, "unauthorized", "请先登录")
			return
		}

		var req updateProfileRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}

		displayName := platform.NormalizeText(req.DisplayName)
		if displayName == "" {
			displayName = authUser.DisplayName
		}
		avatarURL := platform.NormalizeText(req.AvatarURL)
		if avatarURL == "" {
			avatarURL = authUser.AvatarURL
		}

		user, err := dataStore.UpdateUserProfile(r.Context(), authUser.ID, store.UpdateUserProfileInput{
			DisplayName: displayName,
			AvatarURL:   avatarURL,
		})
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "更新用户资料失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, user)
	}
}

func UploadMyAvatar(appRuntime *runtime.Runtime) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		const maxAvatarBytes = 2 << 20

		authUser, ok := platform.AuthUserFromContext(r.Context())
		if !ok {
			platform.WriteError(w, r, http.StatusUnauthorized, "unauthorized", "请先登录")
			return
		}

		r.Body = http.MaxBytesReader(w, r.Body, maxAvatarBytes+(512<<10))
		if err := r.ParseMultipartForm(maxAvatarBytes + (512 << 10)); err != nil {
			if strings.Contains(err.Error(), "request body too large") {
				platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "头像文件不能超过 2MB")
				return
			}
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "头像上传表单解析失败")
			return
		}

		file, header, err := r.FormFile("avatar")
		if err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "缺少头像文件")
			return
		}
		defer file.Close()

		if header.Size > maxAvatarBytes {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "头像文件不能超过 2MB")
			return
		}

		data, err := io.ReadAll(io.LimitReader(file, maxAvatarBytes+1))
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "读取头像文件失败")
			return
		}
		if len(data) > maxAvatarBytes {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "头像文件不能超过 2MB")
			return
		}

		contentType := http.DetectContentType(data)
		ext := avatarExtension(contentType, header.Filename)
		if ext == "" {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "仅支持 PNG、JPG、WEBP、GIF")
			return
		}

		if err := os.MkdirAll(appRuntime.Config.Upload.Dir, 0o755); err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "创建上传目录失败")
			return
		}

		fileName := fmt.Sprintf("avatar-%s-%d%s", authUser.ID, time.Now().UnixNano(), ext)
		fullPath := filepath.Join(appRuntime.Config.Upload.Dir, fileName)
		dst, err := os.OpenFile(fullPath, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o644)
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "保存头像失败")
			return
		}
		defer dst.Close()

		if _, err := dst.Write(data); err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "写入头像失败")
			return
		}

		avatarURL := "/uploads/" + fileName
		user, err := appRuntime.Store.UpdateUserProfile(r.Context(), authUser.ID, store.UpdateUserProfileInput{
			DisplayName: authUser.DisplayName,
			AvatarURL:   avatarURL,
		})
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "更新头像资料失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, user)
	}
}

func middlewareToken(value string) string {
	if value == "" {
		return ""
	}
	const prefix = "Bearer "
	if len(value) >= len(prefix) && value[:len(prefix)] == prefix {
		return value[len(prefix):]
	}
	return value
}

func avatarExtension(contentType string, originalName string) string {
	switch strings.ToLower(contentType) {
	case "image/png":
		return ".png"
	case "image/jpeg", "image/jpg":
		return ".jpg"
	case "image/webp":
		return ".webp"
	case "image/gif":
		return ".gif"
	}

	ext := strings.ToLower(filepath.Ext(originalName))
	switch ext {
	case ".png", ".jpg", ".jpeg", ".webp", ".gif":
		if ext == ".jpeg" {
			return ".jpg"
		}
		return ext
	default:
		return ""
	}
}

package handlers

import (
	"errors"
	"net/http"

	"prts-translation-system/internal/platform"
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

		user, err := dataStore.UpdateUserDisplayName(r.Context(), authUser.ID, displayName)
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "更新用户资料失败")
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

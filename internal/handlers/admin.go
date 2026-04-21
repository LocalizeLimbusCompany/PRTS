package handlers

import (
	"errors"
	"net/http"

	"github.com/go-chi/chi/v5"

	"prts-translation-system/internal/platform"
	"prts-translation-system/internal/store"
)

type updatePlatformSettingsRequest struct {
	AllowUserCreateOrganization bool `json:"allowUserCreateOrganization"`
	AllowUserCreateProject      bool `json:"allowUserCreateProject"`
}

type updatePlatformUserRequest struct {
	PlatformRole string `json:"platformRole"`
	Status       string `json:"status"`
}

func GetPlatformOverview(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		authUser, ok := platform.AuthUserFromContext(r.Context())
		if !ok || (authUser.PlatformRole != "owner" && authUser.PlatformRole != "admin") {
			platform.WriteError(w, r, http.StatusForbidden, "forbidden", "当前用户无权访问管理面板")
			return
		}

		item, err := dataStore.GetPlatformOverview(r.Context())
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取平台概览失败")
			return
		}
		platform.WriteSuccess(w, r, http.StatusOK, item)
	}
}

func GetPlatformSettings(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		authUser, ok := platform.AuthUserFromContext(r.Context())
		if !ok || (authUser.PlatformRole != "owner" && authUser.PlatformRole != "admin") {
			platform.WriteError(w, r, http.StatusForbidden, "forbidden", "当前用户无权访问平台设置")
			return
		}

		item, err := dataStore.GetPlatformSettings(r.Context())
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取平台设置失败")
			return
		}
		platform.WriteSuccess(w, r, http.StatusOK, item)
	}
}

func UpdatePlatformSettings(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		authUser, ok := platform.AuthUserFromContext(r.Context())
		if !ok || (authUser.PlatformRole != "owner" && authUser.PlatformRole != "admin") {
			platform.WriteError(w, r, http.StatusForbidden, "forbidden", "当前用户无权修改平台设置")
			return
		}

		var req updatePlatformSettingsRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}

		item, err := dataStore.UpdatePlatformSettings(r.Context(), store.UpdatePlatformSettingsInput{
			AllowUserCreateOrganization: req.AllowUserCreateOrganization,
			AllowUserCreateProject:      req.AllowUserCreateProject,
			UpdatedBy:                   authUser.ID,
		})
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "更新平台设置失败")
			return
		}
		platform.WriteSuccess(w, r, http.StatusOK, item)
	}
}

func ListPlatformUsers(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		authUser, ok := platform.AuthUserFromContext(r.Context())
		if !ok || (authUser.PlatformRole != "owner" && authUser.PlatformRole != "admin") {
			platform.WriteError(w, r, http.StatusForbidden, "forbidden", "当前用户无权查看用户列表")
			return
		}

		items, err := dataStore.ListUsers(r.Context())
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取用户列表失败")
			return
		}
		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{
			"items": items,
			"total": len(items),
		})
	}
}

func UpdatePlatformUser(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		authUser, ok := platform.AuthUserFromContext(r.Context())
		if !ok || (authUser.PlatformRole != "owner" && authUser.PlatformRole != "admin") {
			platform.WriteError(w, r, http.StatusForbidden, "forbidden", "当前用户无权管理用户")
			return
		}

		userID := chi.URLParam(r, "userId")
		var req updatePlatformUserRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}

		req.PlatformRole = platform.NormalizeIdentifier(req.PlatformRole)
		req.Status = platform.NormalizeIdentifier(req.Status)
		if req.PlatformRole != "owner" && req.PlatformRole != "admin" && req.PlatformRole != "user" {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "platformRole 不合法")
			return
		}
		if req.Status != "active" && req.Status != "disabled" {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "status 不合法")
			return
		}
		if authUser.PlatformRole != "owner" && req.PlatformRole == "owner" {
			platform.WriteError(w, r, http.StatusForbidden, "forbidden", "只有 Owner 可以设置 Owner")
			return
		}

		target, err := dataStore.GetUserByID(r.Context(), userID)
		if errors.Is(err, store.ErrNotFound) {
			platform.WriteError(w, r, http.StatusNotFound, "not_found", "用户不存在")
			return
		}
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取用户失败")
			return
		}
		if target.PlatformRole == "owner" && authUser.PlatformRole != "owner" {
			platform.WriteError(w, r, http.StatusForbidden, "forbidden", "只有 Owner 可以管理 Owner")
			return
		}

		item, err := dataStore.UpdatePlatformUser(r.Context(), userID, store.UpdatePlatformUserInput{
			PlatformRole: req.PlatformRole,
			Status:       req.Status,
		})
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "更新用户失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, item)
	}
}

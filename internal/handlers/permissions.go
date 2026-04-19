package handlers

import (
	"net/http"

	"github.com/go-chi/chi/v5"

	"prts-translation-system/internal/platform"
	"prts-translation-system/internal/store"
)

type setPermissionOverrideRequest struct {
	UserID           string `json:"userId"`
	PermissionNodeID string `json:"permissionNodeId"`
	Effect           string `json:"effect"`
}

type setDocumentRuleRequest struct {
	UserID          string `json:"userId"`
	PermissionScope string `json:"permissionScope"`
	MatchType       string `json:"matchType"`
	MatchValue      string `json:"matchValue"`
	Effect          string `json:"effect"`
}

type upsertProjectMemberRequest struct {
	UserID   string `json:"userId"`
	RoleCode string `json:"roleCode"`
}

func ListProjectMembers(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		items, err := dataStore.ListProjectMembers(r.Context(), projectID)
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取项目成员失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{
			"items": items,
			"total": len(items),
		})
	}
}

func UpsertProjectMember(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		var req upsertProjectMemberRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}

		req.UserID = platform.NormalizeText(req.UserID)
		req.RoleCode = platform.NormalizeIdentifier(req.RoleCode)
		if req.UserID == "" || req.RoleCode == "" {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "userId 和 roleCode 不能为空")
			return
		}

		if err := dataStore.UpsertProjectMember(r.Context(), projectID, store.UpsertProjectMemberInput{
			UserID:   req.UserID,
			RoleCode: req.RoleCode,
		}); err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "保存项目成员失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{"ok": true})
	}
}

func ListProjectRoles(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		items, err := dataStore.ListProjectRoles(r.Context(), projectID)
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取项目角色失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{
			"items": items,
			"total": len(items),
		})
	}
}

func ListProjectPermissions(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")

		nodes, err := dataStore.ListPermissionNodes(r.Context())
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取权限节点失败")
			return
		}

		overrides, err := dataStore.ListPermissionOverrides(r.Context(), projectID)
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取权限覆盖失败")
			return
		}

		rules, err := dataStore.ListDocumentRules(r.Context(), projectID)
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取文档范围规则失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{
			"nodes":     nodes,
			"overrides": overrides,
			"rules":     rules,
		})
	}
}

func SetProjectMemberPermissions(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		userIDFromPath := chi.URLParam(r, "userId")
		var req setPermissionOverrideRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}
		if userIDFromPath != "" {
			req.UserID = userIDFromPath
		}

		if req.UserID == "" || req.PermissionNodeID == "" || (req.Effect != "allow" && req.Effect != "deny") {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "userId、permissionNodeId、effect 不合法")
			return
		}

		if err := dataStore.SetPermissionOverride(r.Context(), projectID, store.SetPermissionOverrideInput{
			UserID:           req.UserID,
			PermissionNodeID: req.PermissionNodeID,
			Effect:           req.Effect,
		}); err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "设置权限覆盖失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{"ok": true})
	}
}

func DeleteProjectMember(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		userID := chi.URLParam(r, "userId")
		if err := dataStore.DeleteProjectMember(r.Context(), projectID, userID); err != nil {
			if err == store.ErrNotFound {
				platform.WriteError(w, r, http.StatusNotFound, "not_found", "项目成员不存在")
				return
			}
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "删除项目成员失败")
			return
		}
		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{"ok": true})
	}
}

func DeleteProjectMemberPermission(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		userID := chi.URLParam(r, "userId")
		permissionNodeID := r.URL.Query().Get("permissionNodeId")
		if permissionNodeID == "" {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "permissionNodeId 不能为空")
			return
		}
		if err := dataStore.DeletePermissionOverride(r.Context(), projectID, userID, permissionNodeID); err != nil {
			if err == store.ErrNotFound {
				platform.WriteError(w, r, http.StatusNotFound, "not_found", "权限覆盖不存在")
				return
			}
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "删除权限覆盖失败")
			return
		}
		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{"ok": true})
	}
}

func SetProjectMemberDocumentRules(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		userIDFromPath := chi.URLParam(r, "userId")
		var req setDocumentRuleRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}
		if userIDFromPath != "" {
			req.UserID = userIDFromPath
		}

		req.PermissionScope = platform.NormalizeIdentifier(req.PermissionScope)
		req.MatchType = platform.NormalizeIdentifier(req.MatchType)
		req.MatchValue = platform.NormalizeText(req.MatchValue)
		req.Effect = platform.NormalizeIdentifier(req.Effect)

		if req.UserID == "" || req.PermissionScope == "" || req.MatchType == "" || req.MatchValue == "" || (req.Effect != "allow" && req.Effect != "deny") {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "userId、permissionScope、matchType、matchValue、effect 不合法")
			return
		}

		if err := dataStore.SetDocumentRule(r.Context(), projectID, store.SetDocumentRuleInput{
			UserID:          req.UserID,
			PermissionScope: req.PermissionScope,
			MatchType:       req.MatchType,
			MatchValue:      req.MatchValue,
			Effect:          req.Effect,
		}); err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "设置文档范围规则失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{"ok": true})
	}
}

func DeleteProjectMemberDocumentRule(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		ruleID := r.URL.Query().Get("ruleId")
		if ruleID == "" {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "ruleId 不能为空")
			return
		}
		if err := dataStore.DeleteDocumentRule(r.Context(), projectID, ruleID); err != nil {
			if err == store.ErrNotFound {
				platform.WriteError(w, r, http.StatusNotFound, "not_found", "文档范围规则不存在")
				return
			}
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "删除文档范围规则失败")
			return
		}
		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{"ok": true})
	}
}

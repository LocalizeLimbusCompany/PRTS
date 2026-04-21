package handlers

import (
	"errors"
	"net/http"

	"github.com/go-chi/chi/v5"

	"prts-translation-system/internal/platform"
	"prts-translation-system/internal/store"
)

type updateUnitRequest struct {
	TargetText string `json:"targetText"`
	Status     string `json:"status"`
	Comment    string `json:"comment"`
}

type reviewUnitRequest struct {
	Status  string `json:"status"`
	Comment string `json:"comment"`
}

func ListTranslationUnits(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		authUser, _ := platform.AuthUserFromContext(r.Context())
		page := platform.NormalizePage(platform.QueryInt(r, "page", 1))
		pageSize := platform.NormalizePageSize(platform.QueryInt(r, "pageSize", 50))

		items, total, err := dataStore.ListTranslationUnits(r.Context(), store.UnitListFilter{
			ActorID:      authUser.ID,
			ProjectID:    projectID,
			DocumentID:   r.URL.Query().Get("documentId"),
			DocumentPath: r.URL.Query().Get("documentPath"),
			Tag:          r.URL.Query().Get("tag"),
			Key:          r.URL.Query().Get("key"),
			Status:       r.URL.Query().Get("status"),
			SourceText:   r.URL.Query().Get("sourceText"),
			TargetText:   r.URL.Query().Get("targetText"),
			UpdatedBy:    r.URL.Query().Get("updatedBy"),
			Page:         page,
			PageSize:     pageSize,
		})
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取翻译条目列表失败")
			return
		}
		if items == nil {
			items = []store.TranslationUnit{}
		}

		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{
			"items":    items,
			"page":     page,
			"pageSize": pageSize,
			"total":    total,
		})
	}
}

func UpdateTranslationUnit(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		unitID := chi.URLParam(r, "unitId")
		var req updateUnitRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}
		if req.Status == "" {
			req.Status = "translated"
		}
		authUser, _ := platform.AuthUserFromContext(r.Context())

		item, err := dataStore.UpdateTranslationUnit(r.Context(), projectID, unitID, store.UpdateTranslationUnitInput{
			TargetText: req.TargetText,
			Status:     req.Status,
			Comment:    req.Comment,
			ActorID:    authUser.ID,
			ChangeNote: req.Comment,
		})
		if errors.Is(err, store.ErrNotFound) {
			platform.WriteError(w, r, http.StatusNotFound, "not_found", "翻译条目不存在")
			return
		}
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "更新翻译条目失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, item)
	}
}

func GetTranslationUnit(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		unitID := chi.URLParam(r, "unitId")
		authUser, _ := platform.AuthUserFromContext(r.Context())

		item, err := dataStore.GetTranslationUnit(r.Context(), projectID, unitID, authUser.ID)
		if errors.Is(err, store.ErrNotFound) {
			platform.WriteError(w, r, http.StatusNotFound, "not_found", "翻译条目不存在")
			return
		}
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取翻译条目详情失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, item)
	}
}

func ReviewTranslationUnit(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		unitID := chi.URLParam(r, "unitId")
		var req reviewUnitRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}
		status := req.Status
		if status == "" {
			status = "reviewed"
		}
		authUser, _ := platform.AuthUserFromContext(r.Context())

		current, err := dataStore.GetTranslationUnit(r.Context(), projectID, unitID, authUser.ID)
		if errors.Is(err, store.ErrNotFound) {
			platform.WriteError(w, r, http.StatusNotFound, "not_found", "翻译条目不存在")
			return
		}
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取翻译条目失败")
			return
		}

		item, err := dataStore.UpdateTranslationUnit(r.Context(), projectID, unitID, store.UpdateTranslationUnitInput{
			TargetText: current.Target.Text,
			Status:     status,
			Comment:    req.Comment,
			ActorID:    authUser.ID,
			ChangeNote: req.Comment,
		})
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "校对翻译条目失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, item)
	}
}

func ApproveTranslationUnit(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		unitID := chi.URLParam(r, "unitId")
		var req reviewUnitRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}
		status := req.Status
		if status == "" {
			status = "approved"
		}
		authUser, _ := platform.AuthUserFromContext(r.Context())

		current, err := dataStore.GetTranslationUnit(r.Context(), projectID, unitID, authUser.ID)
		if errors.Is(err, store.ErrNotFound) {
			platform.WriteError(w, r, http.StatusNotFound, "not_found", "翻译条目不存在")
			return
		}
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取翻译条目失败")
			return
		}

		item, err := dataStore.UpdateTranslationUnit(r.Context(), projectID, unitID, store.UpdateTranslationUnitInput{
			TargetText: current.Target.Text,
			Status:     status,
			Comment:    req.Comment,
			ActorID:    authUser.ID,
			ChangeNote: req.Comment,
		})
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "批准翻译条目失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, item)
	}
}

func ListTranslationUnitHistory(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		unitID := chi.URLParam(r, "unitId")

		items, err := dataStore.ListTranslationUnitHistory(r.Context(), projectID, unitID)
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取翻译历史失败")
			return
		}
		if items == nil {
			items = []store.TranslationRevision{}
		}

		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{
			"items": items,
			"total": len(items),
		})
	}
}

func ListProjectHistory(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		page := platform.NormalizePage(platform.QueryInt(r, "page", 1))
		pageSize := platform.NormalizePageSize(platform.QueryInt(r, "pageSize", 50))

		items, total, err := dataStore.ListProjectHistory(r.Context(), store.ProjectHistoryFilter{
			ProjectID:  projectID,
			DocumentID: r.URL.Query().Get("documentId"),
			Key:        r.URL.Query().Get("key"),
			Status:     r.URL.Query().Get("status"),
			Page:       page,
			PageSize:   pageSize,
		})
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取项目历史失败")
			return
		}
		if items == nil {
			items = []store.ProjectHistoryItem{}
		}

		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{
			"items":    items,
			"total":    total,
			"page":     page,
			"pageSize": pageSize,
		})
	}
}

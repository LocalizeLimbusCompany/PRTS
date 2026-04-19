package handlers

import (
	"errors"
	"net/http"

	"github.com/go-chi/chi/v5"

	"prts-translation-system/internal/platform"
	"prts-translation-system/internal/store"
)

type createTagRequest struct {
	Code      string `json:"code"`
	Name      string `json:"name"`
	Color     string `json:"color"`
	IsVisible *bool  `json:"isVisible"`
}

type updateTagRequest struct {
	Name      string `json:"name"`
	Color     string `json:"color"`
	IsVisible *bool  `json:"isVisible"`
}

type bindTagRequest struct {
	TagID string `json:"tagId"`
}

type createDocumentRequest struct {
	Path      string `json:"path"`
	Title     string `json:"title"`
	UpdatedBy string `json:"updatedBy"`
}

type updateDocumentRequest struct {
	Title     string `json:"title"`
	UpdatedBy string `json:"updatedBy"`
}

func ListTags(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		items, err := dataStore.ListTags(r.Context(), projectID)
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取标签列表失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{
			"items": items,
			"total": len(items),
		})
	}
}

func CreateTag(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		var req createTagRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}
		if req.Code == "" || req.Name == "" {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "code 和 name 不能为空")
			return
		}
		color := req.Color
		if color == "" {
			color = "#4C88FF"
		}
		isVisible := true
		if req.IsVisible != nil {
			isVisible = *req.IsVisible
		}

		item, err := dataStore.CreateTag(r.Context(), projectID, store.CreateTagInput{
			Code:      req.Code,
			Name:      req.Name,
			Color:     color,
			IsVisible: isVisible,
		})
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "创建标签失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusCreated, item)
	}
}

func UpdateTag(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		tagID := chi.URLParam(r, "tagId")
		var req updateTagRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}
		if req.Name == "" {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "name 不能为空")
			return
		}
		color := req.Color
		if color == "" {
			color = "#4C88FF"
		}
		isVisible := true
		if req.IsVisible != nil {
			isVisible = *req.IsVisible
		}

		item, err := dataStore.UpdateTag(r.Context(), projectID, tagID, store.UpdateTagInput{
			Name:      req.Name,
			Color:     color,
			IsVisible: isVisible,
		})
		if errors.Is(err, store.ErrNotFound) {
			platform.WriteError(w, r, http.StatusNotFound, "not_found", "标签不存在")
			return
		}
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "更新标签失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, item)
	}
}

func BindTagToDocument(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		documentID := chi.URLParam(r, "documentId")
		var req bindTagRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}
		if req.TagID == "" {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "tagId 不能为空")
			return
		}

		if err := dataStore.BindTagToDocument(r.Context(), projectID, documentID, req.TagID); err != nil {
			if errors.Is(err, store.ErrNotFound) {
				platform.WriteError(w, r, http.StatusNotFound, "not_found", "文档或标签不存在")
				return
			}
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "绑定标签失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{"ok": true})
	}
}

func UnbindTagFromDocument(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		documentID := chi.URLParam(r, "documentId")
		tagID := chi.URLParam(r, "tagId")

		if err := dataStore.UnbindTagFromDocument(r.Context(), projectID, documentID, tagID); err != nil {
			if errors.Is(err, store.ErrNotFound) {
				platform.WriteError(w, r, http.StatusNotFound, "not_found", "文档标签绑定不存在")
				return
			}
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "解绑标签失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{"ok": true})
	}
}

func ListDocuments(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		page := platform.NormalizePage(platform.QueryInt(r, "page", 1))
		pageSize := platform.NormalizePageSize(platform.QueryInt(r, "pageSize", 50))

		items, total, err := dataStore.ListDocuments(r.Context(), store.DocumentListFilter{
			ProjectID: projectID,
			Path:      r.URL.Query().Get("path"),
			Tag:       r.URL.Query().Get("tag"),
			UpdatedBy: r.URL.Query().Get("updatedBy"),
			Page:      page,
			PageSize:  pageSize,
		})
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取文档列表失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{
			"items":    items,
			"page":     page,
			"pageSize": pageSize,
			"total":    total,
		})
	}
}

func CreateDocument(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		var req createDocumentRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}

		path := platform.NormalizeText(req.Path)
		title := platform.NormalizeText(req.Title)
		if path == "" {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "path 不能为空")
			return
		}
		if title == "" {
			title = path
		}

		item, err := dataStore.CreateDocument(r.Context(), projectID, store.CreateDocumentInput{
			Path:      path,
			Title:     title,
			UpdatedBy: platform.NormalizeText(req.UpdatedBy),
		})
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "创建文档失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusCreated, item)
	}
}

func UpdateDocument(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		documentID := chi.URLParam(r, "documentId")
		var req updateDocumentRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}

		title := platform.NormalizeText(req.Title)
		if title == "" {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "title 不能为空")
			return
		}

		item, err := dataStore.UpdateDocument(r.Context(), projectID, documentID, store.UpdateDocumentInput{
			Title:     title,
			UpdatedBy: platform.NormalizeText(req.UpdatedBy),
		})
		if errors.Is(err, store.ErrNotFound) {
			platform.WriteError(w, r, http.StatusNotFound, "not_found", "文档不存在")
			return
		}
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "更新文档失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, item)
	}
}

func GetDocument(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		documentID := chi.URLParam(r, "documentId")

		item, err := dataStore.GetDocument(r.Context(), projectID, documentID)
		if errors.Is(err, store.ErrNotFound) {
			platform.WriteError(w, r, http.StatusNotFound, "not_found", "文档不存在")
			return
		}
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取文档详情失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, item)
	}
}

func ListDocumentVersions(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		documentID := chi.URLParam(r, "documentId")

		items, err := dataStore.ListDocumentVersions(r.Context(), projectID, documentID)
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取文档版本失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{
			"items": items,
			"total": len(items),
		})
	}
}

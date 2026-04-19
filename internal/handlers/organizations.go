package handlers

import (
	"errors"
	"net/http"

	"github.com/go-chi/chi/v5"

	"prts-translation-system/internal/platform"
	"prts-translation-system/internal/store"
)

type createOrganizationRequest struct {
	Slug        string `json:"slug"`
	Name        string `json:"name"`
	Description string `json:"description"`
	Visibility  string `json:"visibility"`
	CreatedBy   string `json:"createdBy"`
}

type updateOrganizationRequest struct {
	Name        string `json:"name"`
	Description string `json:"description"`
	Visibility  string `json:"visibility"`
}

func ListOrganizations(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		items, err := dataStore.ListOrganizations(r.Context())
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取组织列表失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{
			"items": items,
			"total": len(items),
		})
	}
}

func GetOrganization(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		organizationID := chi.URLParam(r, "organizationId")
		item, err := dataStore.GetOrganization(r.Context(), organizationID)
		if errors.Is(err, store.ErrNotFound) {
			platform.WriteError(w, r, http.StatusNotFound, "not_found", "组织不存在")
			return
		}
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取组织详情失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, item)
	}
}

func CreateOrganization(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		var req createOrganizationRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}

		slug := platform.NormalizeIdentifier(req.Slug)
		name := platform.NormalizeText(req.Name)
		description := platform.NormalizeText(req.Description)
		visibility := platform.NormalizeIdentifier(req.Visibility)
		if visibility == "" {
			visibility = "public"
		}

		if slug == "" || name == "" {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "slug 和 name 不能为空")
			return
		}

		item, err := dataStore.CreateOrganization(r.Context(), store.CreateOrganizationInput{
			Slug:        slug,
			Name:        name,
			Description: description,
			Visibility:  visibility,
			CreatedBy:   platform.NormalizeText(req.CreatedBy),
		})
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "创建组织失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusCreated, item)
	}
}

func UpdateOrganization(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		organizationID := chi.URLParam(r, "organizationId")
		var req updateOrganizationRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}

		name := platform.NormalizeText(req.Name)
		description := platform.NormalizeText(req.Description)
		visibility := platform.NormalizeIdentifier(req.Visibility)
		if visibility == "" {
			visibility = "public"
		}

		if name == "" {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "name 不能为空")
			return
		}

		item, err := dataStore.UpdateOrganization(r.Context(), organizationID, store.UpdateOrganizationInput{
			Name:        name,
			Description: description,
			Visibility:  visibility,
		})
		if errors.Is(err, store.ErrNotFound) {
			platform.WriteError(w, r, http.StatusNotFound, "not_found", "组织不存在")
			return
		}
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "更新组织失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, item)
	}
}

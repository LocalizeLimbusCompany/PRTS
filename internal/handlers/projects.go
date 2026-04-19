package handlers

import (
	"errors"
	"net/http"

	"github.com/go-chi/chi/v5"

	"prts-translation-system/internal/platform"
	"prts-translation-system/internal/store"
)

type createProjectRequest struct {
	Slug            string   `json:"slug"`
	Name            string   `json:"name"`
	Description     string   `json:"description"`
	TargetLanguage  string   `json:"targetLanguage"`
	SourceLanguages []string `json:"sourceLanguages"`
	Visibility      string   `json:"visibility"`
	GuestPolicy     string   `json:"guestPolicy"`
	CreatedBy       string   `json:"createdBy"`
}

type updateProjectRequest struct {
	Name            string   `json:"name"`
	Description     string   `json:"description"`
	TargetLanguage  string   `json:"targetLanguage"`
	SourceLanguages []string `json:"sourceLanguages"`
	Visibility      string   `json:"visibility"`
	GuestPolicy     string   `json:"guestPolicy"`
}

func ListProjectsByOrganization(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		organizationID := chi.URLParam(r, "organizationId")
		items, err := dataStore.ListProjectsByOrganization(r.Context(), organizationID)
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取项目列表失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{
			"items": items,
			"total": len(items),
		})
	}
}

func GetProject(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		item, err := dataStore.GetProject(r.Context(), projectID)
		if errors.Is(err, store.ErrNotFound) {
			platform.WriteError(w, r, http.StatusNotFound, "not_found", "项目不存在")
			return
		}
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取项目详情失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, item)
	}
}

func CreateProject(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		organizationID := chi.URLParam(r, "organizationId")
		var req createProjectRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}

		slug := platform.NormalizeIdentifier(req.Slug)
		name := platform.NormalizeText(req.Name)
		description := platform.NormalizeText(req.Description)
		targetLanguage := platform.NormalizeIdentifier(req.TargetLanguage)
		sourceLanguages := platform.NormalizeLanguageCodes(req.SourceLanguages)
		visibility := platform.NormalizeIdentifier(req.Visibility)
		guestPolicy := platform.NormalizeIdentifier(req.GuestPolicy)
		if visibility == "" {
			visibility = "public"
		}
		if guestPolicy == "" {
			guestPolicy = "read"
		}

		if slug == "" || name == "" || targetLanguage == "" || len(sourceLanguages) == 0 {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "slug、name、targetLanguage、sourceLanguages 不能为空")
			return
		}

		item, err := dataStore.CreateProject(r.Context(), organizationID, store.CreateProjectInput{
			Slug:            slug,
			Name:            name,
			Description:     description,
			TargetLanguage:  targetLanguage,
			SourceLanguages: sourceLanguages,
			Visibility:      visibility,
			GuestPolicy:     guestPolicy,
			CreatedBy:       platform.NormalizeText(req.CreatedBy),
		})
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "创建项目失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusCreated, item)
	}
}

func UpdateProject(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		var req updateProjectRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}

		name := platform.NormalizeText(req.Name)
		description := platform.NormalizeText(req.Description)
		targetLanguage := platform.NormalizeIdentifier(req.TargetLanguage)
		sourceLanguages := platform.NormalizeLanguageCodes(req.SourceLanguages)
		visibility := platform.NormalizeIdentifier(req.Visibility)
		guestPolicy := platform.NormalizeIdentifier(req.GuestPolicy)
		if visibility == "" {
			visibility = "public"
		}
		if guestPolicy == "" {
			guestPolicy = "read"
		}

		if name == "" || targetLanguage == "" || len(sourceLanguages) == 0 {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "name、targetLanguage、sourceLanguages 不能为空")
			return
		}

		item, err := dataStore.UpdateProject(r.Context(), projectID, store.UpdateProjectInput{
			Name:            name,
			Description:     description,
			TargetLanguage:  targetLanguage,
			SourceLanguages: sourceLanguages,
			Visibility:      visibility,
			GuestPolicy:     guestPolicy,
		})
		if errors.Is(err, store.ErrNotFound) {
			platform.WriteError(w, r, http.StatusNotFound, "not_found", "项目不存在")
			return
		}
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "更新项目失败")
			return
		}

		platform.WriteSuccess(w, r, http.StatusOK, item)
	}
}

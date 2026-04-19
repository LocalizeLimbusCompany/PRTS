package handlers

import (
	"archive/zip"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"os"
	"path/filepath"

	"github.com/go-chi/chi/v5"

	"prts-translation-system/internal/platform"
	"prts-translation-system/internal/runtime"
	"prts-translation-system/internal/store"
)

type glossaryTermRequest struct {
	SourceTerm     string `json:"sourceTerm"`
	TargetTerm     string `json:"targetTerm"`
	SourceLanguage string `json:"sourceLanguage"`
	TargetLanguage string `json:"targetLanguage"`
	Note           string `json:"note"`
}

type tmEntryRequest struct {
	SourceLanguage string `json:"sourceLanguage"`
	TargetLanguage string `json:"targetLanguage"`
	SourceText     string `json:"sourceText"`
	TargetText     string `json:"targetText"`
	QualityScore   int    `json:"qualityScore"`
}

func ListGlossaryTerms(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		items, err := dataStore.ListGlossaryTerms(r.Context(), projectID)
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取术语库失败")
			return
		}
		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{"items": items, "total": len(items)})
	}
}

func CreateGlossaryTerm(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		authUser, _ := platform.AuthUserFromContext(r.Context())
		var req glossaryTermRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}
		item, err := dataStore.CreateGlossaryTerm(r.Context(), projectID, store.CreateGlossaryTermInput{
			SourceTerm:     platform.NormalizeText(req.SourceTerm),
			TargetTerm:     platform.NormalizeText(req.TargetTerm),
			SourceLanguage: platform.NormalizeIdentifier(req.SourceLanguage),
			TargetLanguage: platform.NormalizeIdentifier(req.TargetLanguage),
			Note:           platform.NormalizeText(req.Note),
			CreatedBy:      authUser.ID,
		})
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "创建术语失败")
			return
		}
		platform.WriteSuccess(w, r, http.StatusCreated, item)
	}
}

func UpdateGlossaryTerm(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		termID := chi.URLParam(r, "termId")
		var req glossaryTermRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}
		item, err := dataStore.UpdateGlossaryTerm(r.Context(), projectID, termID, store.UpdateGlossaryTermInput{
			SourceTerm:     platform.NormalizeText(req.SourceTerm),
			TargetTerm:     platform.NormalizeText(req.TargetTerm),
			SourceLanguage: platform.NormalizeIdentifier(req.SourceLanguage),
			TargetLanguage: platform.NormalizeIdentifier(req.TargetLanguage),
			Note:           platform.NormalizeText(req.Note),
		})
		if errors.Is(err, store.ErrNotFound) {
			platform.WriteError(w, r, http.StatusNotFound, "not_found", "术语不存在")
			return
		}
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "更新术语失败")
			return
		}
		platform.WriteSuccess(w, r, http.StatusOK, item)
	}
}

func DeleteGlossaryTerm(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		termID := chi.URLParam(r, "termId")
		if err := dataStore.DeleteGlossaryTerm(r.Context(), projectID, termID); err != nil {
			if errors.Is(err, store.ErrNotFound) {
				platform.WriteError(w, r, http.StatusNotFound, "not_found", "术语不存在")
				return
			}
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "删除术语失败")
			return
		}
		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{"ok": true})
	}
}

func ListTMEntries(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		items, err := dataStore.ListTMEntries(r.Context(), projectID)
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取翻译记忆失败")
			return
		}
		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{"items": items, "total": len(items)})
	}
}

func CreateTMEntry(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		var req tmEntryRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}
		item, err := dataStore.CreateTMEntry(r.Context(), projectID, store.CreateTMEntryInput{
			SourceLanguage: platform.NormalizeIdentifier(req.SourceLanguage),
			TargetLanguage: platform.NormalizeIdentifier(req.TargetLanguage),
			SourceText:     platform.NormalizeText(req.SourceText),
			TargetText:     platform.NormalizeText(req.TargetText),
			QualityScore:   req.QualityScore,
		})
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "创建翻译记忆失败")
			return
		}
		platform.WriteSuccess(w, r, http.StatusCreated, item)
	}
}

func UpdateTMEntry(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		entryID := chi.URLParam(r, "entryId")
		var req tmEntryRequest
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}
		item, err := dataStore.UpdateTMEntry(r.Context(), projectID, entryID, store.UpdateTMEntryInput{
			SourceLanguage: platform.NormalizeIdentifier(req.SourceLanguage),
			TargetLanguage: platform.NormalizeIdentifier(req.TargetLanguage),
			SourceText:     platform.NormalizeText(req.SourceText),
			TargetText:     platform.NormalizeText(req.TargetText),
			QualityScore:   req.QualityScore,
		})
		if errors.Is(err, store.ErrNotFound) {
			platform.WriteError(w, r, http.StatusNotFound, "not_found", "翻译记忆不存在")
			return
		}
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "更新翻译记忆失败")
			return
		}
		platform.WriteSuccess(w, r, http.StatusOK, item)
	}
}

func DeleteTMEntry(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		entryID := chi.URLParam(r, "entryId")
		if err := dataStore.DeleteTMEntry(r.Context(), projectID, entryID); err != nil {
			if errors.Is(err, store.ErrNotFound) {
				platform.WriteError(w, r, http.StatusNotFound, "not_found", "翻译记忆不存在")
				return
			}
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "删除翻译记忆失败")
			return
		}
		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{"ok": true})
	}
}

func ListImportJobs(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		items, err := dataStore.ListImportJobs(r.Context(), projectID)
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取导入任务失败")
			return
		}
		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{"items": items, "total": len(items)})
	}
}

func CreateImportJob(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		authUser, _ := platform.AuthUserFromContext(r.Context())
		var req store.ImportDocumentPayload
		if err := platform.DecodeJSON(r, &req); err != nil {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "请求体格式不正确")
			return
		}
		req.Document = platform.NormalizeText(req.Document)
		if req.Document == "" {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "document 不能为空")
			return
		}
		if len(req.Entries) == 0 {
			platform.WriteError(w, r, http.StatusBadRequest, "validation_error", "entries 不能为空")
			return
		}
		item, err := dataStore.ImportDocument(r.Context(), projectID, authUser.ID, req)
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "导入文档失败")
			return
		}
		platform.WriteSuccess(w, r, http.StatusCreated, item)
	}
}

func ListExportJobs(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		items, err := dataStore.ListExportJobs(r.Context(), projectID)
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取导出任务失败")
			return
		}
		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{"items": items, "total": len(items)})
	}
}

func CreateExportJob(runtime *runtime.Runtime) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		authUser, _ := platform.AuthUserFromContext(r.Context())
		item, err := runtime.Store.CreateExportJob(r.Context(), projectID, authUser.ID)
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "创建导出任务失败")
			return
		}

		docs, err := runtime.Store.ExportProjectDocuments(r.Context(), projectID)
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "读取导出文档失败")
			return
		}

		filePath, fileSize, err := writeExportZip(runtime.Config.Export.Dir, projectID, item.ID, docs)
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "生成导出文件失败")
			return
		}

		if err := runtime.Store.UpdateExportJobFile(r.Context(), projectID, item.ID, filePath, fileSize); err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "更新导出任务失败")
			return
		}

		item, err = runtime.Store.GetExportJob(r.Context(), projectID, item.ID)
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取导出任务失败")
			return
		}
		platform.WriteSuccess(w, r, http.StatusCreated, item)
	}
}

func DownloadExportJob(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		exportJobID := chi.URLParam(r, "exportJobId")
		item, err := dataStore.GetExportJob(r.Context(), projectID, exportJobID)
		if errors.Is(err, store.ErrNotFound) {
			platform.WriteError(w, r, http.StatusNotFound, "not_found", "导出任务不存在")
			return
		}
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取导出任务失败")
			return
		}
		if item.FilePath == "" {
			platform.WriteError(w, r, http.StatusNotFound, "not_found", "导出文件尚未生成")
			return
		}
		http.ServeFile(w, r, item.FilePath)
	}
}

func ListAuditLogs(dataStore *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		projectID := chi.URLParam(r, "projectId")
		items, err := dataStore.ListAuditLogs(r.Context(), projectID)
		if err != nil {
			platform.WriteError(w, r, http.StatusInternalServerError, "internal_error", "获取审计日志失败")
			return
		}
		platform.WriteSuccess(w, r, http.StatusOK, map[string]any{"items": items, "total": len(items)})
	}
}

func writeExportZip(exportDir, projectID, exportJobID string, docs []store.ExportDocumentPayload) (string, int64, error) {
	if err := os.MkdirAll(exportDir, 0o755); err != nil {
		return "", 0, err
	}

	filePath := filepath.Join(exportDir, fmt.Sprintf("%s-%s.zip", projectID, exportJobID))
	file, err := os.Create(filePath)
	if err != nil {
		return "", 0, err
	}
	defer file.Close()

	zipWriter := zip.NewWriter(file)
	for _, doc := range docs {
		writer, err := zipWriter.Create(doc.Document)
		if err != nil {
			_ = zipWriter.Close()
			return "", 0, err
		}
		payload, err := json.MarshalIndent(doc, "", "  ")
		if err != nil {
			_ = zipWriter.Close()
			return "", 0, err
		}
		if _, err := writer.Write(payload); err != nil {
			_ = zipWriter.Close()
			return "", 0, err
		}
	}
	if err := zipWriter.Close(); err != nil {
		return "", 0, err
	}

	info, err := os.Stat(filePath)
	if err != nil {
		return "", 0, err
	}
	return filePath, info.Size(), nil
}

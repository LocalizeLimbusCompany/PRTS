package handlers

import (
	"net/http"

	"prts-translation-system/internal/config"
	"prts-translation-system/internal/platform"
)

func APIIndex(cfg config.Config) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		platform.WriteJSON(w, http.StatusOK, map[string]any{
			"success": true,
			"data": map[string]any{
				"name":        cfg.App.Name,
				"environment": cfg.App.Env,
				"version":     "dev",
				"apiPrefix":   "/api/v1",
			},
			"error":     nil,
			"requestId": platform.RequestIDFromRequest(r),
		})
	}
}

func Meta(cfg config.Config) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		platform.WriteJSON(w, http.StatusOK, map[string]any{
			"success": true,
			"data": map[string]any{
				"appName":              cfg.App.Name,
				"platformI18n":         true,
				"defaultFrontendPort":  13000,
				"defaultBackendPort":   cfg.API.Port,
				"documentTagSupported": true,
				"projectExportFormat":  "zip-with-json-files",
			},
			"error":     nil,
			"requestId": platform.RequestIDFromRequest(r),
		})
	}
}

package handlers

import (
	"net/http"

	"prts-translation-system/internal/platform"
)

func Health(w http.ResponseWriter, r *http.Request) {
	platform.WriteJSON(w, http.StatusOK, map[string]any{
		"success": true,
		"data": map[string]any{
			"status": "ok",
		},
		"error":     nil,
		"requestId": platform.RequestIDFromRequest(r),
	})
}

func Ready(w http.ResponseWriter, r *http.Request) {
	platform.WriteJSON(w, http.StatusOK, map[string]any{
		"success": true,
		"data": map[string]any{
			"status": "ready",
		},
		"error":     nil,
		"requestId": platform.RequestIDFromRequest(r),
	})
}

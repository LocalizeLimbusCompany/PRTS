package platform

import "net/http"

type ErrorBody struct {
	Code    string `json:"code"`
	Message string `json:"message"`
}

func WriteSuccess(w http.ResponseWriter, r *http.Request, status int, data any) {
	WriteJSON(w, status, map[string]any{
		"success":   true,
		"data":      data,
		"error":     nil,
		"requestId": RequestIDFromRequest(r),
	})
}

func WriteError(w http.ResponseWriter, r *http.Request, status int, code, message string) {
	WriteJSON(w, status, map[string]any{
		"success": false,
		"data":    nil,
		"error": ErrorBody{
			Code:    code,
			Message: message,
		},
		"requestId": RequestIDFromRequest(r),
	})
}

package platform

import (
	"encoding/json"
	"net/http"

	chimiddleware "github.com/go-chi/chi/v5/middleware"
)

func WriteJSON(w http.ResponseWriter, status int, payload any) {
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(payload)
}

func RequestIDFromRequest(r *http.Request) string {
	return chimiddleware.GetReqID(r.Context())
}

package platform

import (
	"net/http"
	"strconv"
)

func QueryInt(r *http.Request, key string, fallback int) int {
	raw := r.URL.Query().Get(key)
	if raw == "" {
		return fallback
	}

	value, err := strconv.Atoi(raw)
	if err != nil {
		return fallback
	}

	return value
}

func NormalizePage(page int) int {
	if page < 1 {
		return 1
	}
	return page
}

func NormalizePageSize(pageSize int) int {
	switch {
	case pageSize < 1:
		return 50
	case pageSize > 200:
		return 200
	default:
		return pageSize
	}
}

package middleware

import (
	"net/http"
	"strings"

	"prts-translation-system/internal/platform"
	"prts-translation-system/internal/store"
)

func OptionalAuth(dataStore *store.Store) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			token := bearerToken(r.Header.Get("Authorization"))
			if token == "" {
				next.ServeHTTP(w, r)
				return
			}

			user, err := dataStore.GetUserByToken(r.Context(), token)
			if err != nil {
				next.ServeHTTP(w, r)
				return
			}

			ctx := platform.WithAuthUser(r.Context(), platform.AuthContextUser{
				ID:                      user.ID,
				Email:                   user.Email,
				Username:                user.Username,
				DisplayName:             user.DisplayName,
				AvatarURL:               user.AvatarURL,
				PlatformRole:            user.PlatformRole,
				PreferredLocale:         user.PreferredLocale,
				PreferredSourceLanguage: user.PreferredSourceLanguage,
				Status:                  user.Status,
			})
			next.ServeHTTP(w, r.WithContext(ctx))
		})
	}
}

func RequireAuth(dataStore *store.Store) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			token := bearerToken(r.Header.Get("Authorization"))
			if token == "" {
				platform.WriteError(w, r, http.StatusUnauthorized, "unauthorized", "请先登录")
				return
			}

			user, err := dataStore.GetUserByToken(r.Context(), token)
			if err != nil {
				platform.WriteError(w, r, http.StatusUnauthorized, "unauthorized", "登录状态无效")
				return
			}

			ctx := platform.WithAuthUser(r.Context(), platform.AuthContextUser{
				ID:                      user.ID,
				Email:                   user.Email,
				Username:                user.Username,
				DisplayName:             user.DisplayName,
				AvatarURL:               user.AvatarURL,
				PlatformRole:            user.PlatformRole,
				PreferredLocale:         user.PreferredLocale,
				PreferredSourceLanguage: user.PreferredSourceLanguage,
				Status:                  user.Status,
			})
			next.ServeHTTP(w, r.WithContext(ctx))
		})
	}
}

func bearerToken(value string) string {
	if value == "" {
		return ""
	}
	if !strings.HasPrefix(strings.ToLower(value), "bearer ") {
		return ""
	}
	return strings.TrimSpace(value[7:])
}

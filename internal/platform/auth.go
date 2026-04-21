package platform

import "context"

type contextKey string

const authUserContextKey contextKey = "authUser"

type AuthContextUser struct {
	ID                      string `json:"id"`
	Email                   string `json:"email"`
	Username                string `json:"username"`
	DisplayName             string `json:"displayName"`
	AvatarURL               string `json:"avatarUrl"`
	PlatformRole            string `json:"platformRole"`
	PreferredLocale         string `json:"preferredLocale"`
	PreferredSourceLanguage string `json:"preferredSourceLanguage"`
	Status                  string `json:"status"`
}

func WithAuthUser(ctx context.Context, user AuthContextUser) context.Context {
	return context.WithValue(ctx, authUserContextKey, user)
}

func AuthUserFromContext(ctx context.Context) (AuthContextUser, bool) {
	user, ok := ctx.Value(authUserContextKey).(AuthContextUser)
	return user, ok
}

package config

import (
	"os"
	"strconv"
	"time"
)

type Config struct {
	App    AppConfig
	API    APIConfig
	DB     DBConfig
	Redis  RedisConfig
	Export ExportConfig
}

type AppConfig struct {
	Name string
	Env  string
}

type APIConfig struct {
	Port int
}

type DBConfig struct {
	URL string
}

type RedisConfig struct {
	URL string
}

type ExportConfig struct {
	Dir             string
	Retention       time.Duration
	CleanupInterval time.Duration
}

func Load() Config {
	return Config{
		App: AppConfig{
			Name: getEnv("APP_NAME", "prts-translation-system"),
			Env:  getEnv("APP_ENV", "development"),
		},
		API: APIConfig{
			Port: getEnvInt("API_PORT", 18080),
		},
		DB: DBConfig{
			URL: getEnv("DATABASE_URL", "postgres://postgres:postgres@localhost:15432/prts_translation_system?sslmode=disable"),
		},
		Redis: RedisConfig{
			URL: getEnv("REDIS_URL", "redis://localhost:16379/0"),
		},
		Export: ExportConfig{
			Dir:             getEnv("EXPORT_DIR", "exports"),
			Retention:       getEnvDuration("EXPORT_RETENTION", 24*time.Hour),
			CleanupInterval: getEnvDuration("EXPORT_CLEANUP_INTERVAL", time.Hour),
		},
	}
}

func getEnv(key, fallback string) string {
	if value, ok := os.LookupEnv(key); ok && value != "" {
		return value
	}
	return fallback
}

func getEnvInt(key string, fallback int) int {
	value := getEnv(key, "")
	if value == "" {
		return fallback
	}

	parsed, err := strconv.Atoi(value)
	if err != nil {
		return fallback
	}

	return parsed
}

func getEnvDuration(key string, fallback time.Duration) time.Duration {
	value := getEnv(key, "")
	if value == "" {
		return fallback
	}

	parsed, err := time.ParseDuration(value)
	if err != nil {
		return fallback
	}

	return parsed
}

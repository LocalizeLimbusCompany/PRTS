package app

import (
	"log"
	"time"

	"prts-translation-system/internal/config"
)

func RunWorker() error {
	cfg := config.Load()
	log.Printf("worker started, export cleanup ttl=%s, cleanup interval=%s", cfg.Export.Retention, cfg.Export.CleanupInterval)

	ticker := time.NewTicker(cfg.Export.CleanupInterval)
	defer ticker.Stop()

	for range ticker.C {
		log.Printf("worker heartbeat: export cleanup would run here")
	}

	return nil
}

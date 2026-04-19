package app

import (
	"context"
	"fmt"
	"net/http"
	"os/signal"
	"syscall"
	"time"

	"prts-translation-system/internal/config"
	"prts-translation-system/internal/httpserver"
	"prts-translation-system/internal/runtime"
)

func RunAPI() error {
	cfg := config.Load()
	appRuntime, err := runtime.New(context.Background(), cfg)
	if err != nil {
		return err
	}
	defer appRuntime.Close()

	server := httpserver.New(cfg, appRuntime)

	httpServer := &http.Server{
		Addr:              fmt.Sprintf(":%d", cfg.API.Port),
		Handler:           server.Handler(),
		ReadHeaderTimeout: 10 * time.Second,
	}

	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()

	errCh := make(chan error, 1)

	go func() {
		errCh <- httpServer.ListenAndServe()
	}()

	select {
	case err := <-errCh:
		if err == http.ErrServerClosed {
			return nil
		}
		return err
	case <-ctx.Done():
		shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		return httpServer.Shutdown(shutdownCtx)
	}
}

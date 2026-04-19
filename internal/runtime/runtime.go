package runtime

import (
	"context"

	"prts-translation-system/internal/config"
	"prts-translation-system/internal/store"
)

type Runtime struct {
	Config config.Config
	Store *store.Store
}

func New(ctx context.Context, cfg config.Config) (*Runtime, error) {
	dataStore, err := store.New(ctx, cfg.DB.URL)
	if err != nil {
		return nil, err
	}

	return &Runtime{
		Config: cfg,
		Store: dataStore,
	}, nil
}

func (r *Runtime) Close() {
	if r == nil {
		return
	}
	if r.Store != nil {
		r.Store.Close()
	}
}

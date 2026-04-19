package main

import (
	"log"

	"prts-translation-system/internal/app"
)

func main() {
	if err := app.RunWorker(); err != nil {
		log.Fatal(err)
	}
}

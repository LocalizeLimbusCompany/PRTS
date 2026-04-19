package main

import (
	"log"

	"prts-translation-system/internal/app"
)

func main() {
	if err := app.RunAPI(); err != nil {
		log.Fatal(err)
	}
}

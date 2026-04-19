package platform

import (
	"encoding/json"
	"errors"
	"net/http"
)

func DecodeJSON(r *http.Request, target any) error {
	decoder := json.NewDecoder(r.Body)
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(target); err != nil {
		return err
	}
	if decoder.More() {
		return errors.New("request body must contain only one JSON object")
	}
	return nil
}

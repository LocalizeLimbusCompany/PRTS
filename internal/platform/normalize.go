package platform

import "strings"

func NormalizeIdentifier(value string) string {
	value = strings.TrimSpace(strings.ToLower(value))
	value = strings.ReplaceAll(value, " ", "-")
	return value
}

func NormalizeText(value string) string {
	return strings.TrimSpace(value)
}

func NormalizeLanguageCodes(values []string) []string {
	result := make([]string, 0, len(values))
	for _, value := range values {
		normalized := NormalizeIdentifier(value)
		if normalized == "" {
			continue
		}
		result = append(result, normalized)
	}
	return result
}

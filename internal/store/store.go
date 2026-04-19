package store

import (
	"context"
	"crypto/rand"
	"encoding/json"
	"errors"
	"fmt"
	"encoding/hex"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"golang.org/x/crypto/bcrypt"
)

var ErrNotFound = errors.New("not found")

type Store struct {
	pool *pgxpool.Pool
}

type UpdateTranslationUnitInput struct {
	TargetText string
	Status     string
	Comment    string
	ActorID    string
	ChangeNote string
}

type CreateTagInput struct {
	Code      string
	Name      string
	Color     string
	IsVisible bool
}

type UpdateTagInput struct {
	Name      string
	Color     string
	IsVisible bool
}

type CreateOrganizationInput struct {
	Slug        string
	Name        string
	Description string
	Visibility  string
	CreatedBy   string
}

type UpdateOrganizationInput struct {
	Name        string
	Description string
	Visibility  string
}

type CreateProjectInput struct {
	Slug            string
	Name            string
	Description     string
	TargetLanguage  string
	SourceLanguages []string
	Visibility      string
	GuestPolicy     string
	CreatedBy       string
}

type UpdateProjectInput struct {
	Name            string
	Description     string
	TargetLanguage  string
	SourceLanguages []string
	Visibility      string
	GuestPolicy     string
}

type CreateDocumentInput struct {
	Path      string
	Title     string
	UpdatedBy string
}

type UpdateDocumentInput struct {
	Title     string
	UpdatedBy string
}

type ProjectMember struct {
	ID          string       `json:"id"`
	User        UserSummary  `json:"user"`
	RoleCode    string       `json:"roleCode"`
	CreatedAt   time.Time    `json:"createdAt"`
}

type PermissionNode struct {
	ID          string `json:"id"`
	Code        string `json:"code"`
	Name        string `json:"name"`
	Description string `json:"description"`
}

type ProjectRole struct {
	ID          string           `json:"id"`
	Code        string           `json:"code"`
	Name        string           `json:"name"`
	IsSystem    bool             `json:"isSystem"`
	Permissions []PermissionNode `json:"permissions"`
}

type PermissionOverride struct {
	ID             string         `json:"id"`
	User           UserSummary    `json:"user"`
	PermissionNode PermissionNode `json:"permissionNode"`
	Effect         string         `json:"effect"`
}

type DocumentRule struct {
	ID              string      `json:"id"`
	User            UserSummary `json:"user"`
	PermissionScope string      `json:"permissionScope"`
	MatchType       string      `json:"matchType"`
	MatchValue      string      `json:"matchValue"`
	Effect          string      `json:"effect"`
	CreatedAt       time.Time   `json:"createdAt"`
}

type SetPermissionOverrideInput struct {
	UserID           string
	PermissionNodeID string
	Effect           string
}

type SetDocumentRuleInput struct {
	UserID          string
	PermissionScope string
	MatchType       string
	MatchValue      string
	Effect          string
}

type UpsertProjectMemberInput struct {
	UserID   string
	RoleCode string
}

type AuthUser struct {
	ID                      string `json:"id"`
	Email                   string `json:"email"`
	Username                string `json:"username"`
	DisplayName             string `json:"displayName"`
	PreferredLocale         string `json:"preferredLocale"`
	PreferredSourceLanguage string `json:"preferredSourceLanguage"`
}

type GlossaryTerm struct {
	ID             string       `json:"id"`
	SourceTerm     string       `json:"sourceTerm"`
	TargetTerm     string       `json:"targetTerm"`
	SourceLanguage string       `json:"sourceLanguage"`
	TargetLanguage string       `json:"targetLanguage"`
	Note           string       `json:"note"`
	CreatedAt      time.Time    `json:"createdAt"`
	CreatedBy      *UserSummary `json:"createdBy,omitempty"`
}

type TranslationMemoryEntry struct {
	ID              string    `json:"id"`
	SourceLanguage  string    `json:"sourceLanguage"`
	TargetLanguage  string    `json:"targetLanguage"`
	SourceText      string    `json:"sourceText"`
	TargetText      string    `json:"targetText"`
	QualityScore    int       `json:"qualityScore"`
	CreatedFromUnitID *string `json:"createdFromUnitId,omitempty"`
	CreatedAt       time.Time `json:"createdAt"`
}

type ImportJob struct {
	ID          string       `json:"id"`
	DocumentPath string      `json:"documentPath"`
	Status      string       `json:"status"`
	ErrorMessage string      `json:"errorMessage"`
	CreatedAt   time.Time    `json:"createdAt"`
	StartedAt   *time.Time   `json:"startedAt,omitempty"`
	FinishedAt  *time.Time   `json:"finishedAt,omitempty"`
	UploadedBy  *UserSummary `json:"uploadedBy,omitempty"`
}

type ExportJob struct {
	ID          string       `json:"id"`
	Status      string       `json:"status"`
	FilePath    string       `json:"filePath"`
	FileName    string       `json:"fileName"`
	FileSize    int64        `json:"fileSize"`
	ExpiresAt   *time.Time   `json:"expiresAt,omitempty"`
	CreatedAt   time.Time    `json:"createdAt"`
	StartedAt   *time.Time   `json:"startedAt,omitempty"`
	FinishedAt  *time.Time   `json:"finishedAt,omitempty"`
	RequestedBy *UserSummary `json:"requestedBy,omitempty"`
}

type AuditLog struct {
	ID           string          `json:"id"`
	Action       string          `json:"action"`
	ResourceType string          `json:"resourceType"`
	ResourceID   string          `json:"resourceId"`
	DetailJSON   json.RawMessage `json:"detailJson"`
	CreatedAt    time.Time       `json:"createdAt"`
	User         *UserSummary    `json:"user,omitempty"`
}

type ImportDocumentPayload struct {
	Document string               `json:"document"`
	Entries  []ImportEntryPayload `json:"entries"`
}

type ImportEntryPayload struct {
	Key    string            `json:"key"`
	Source map[string]string `json:"source"`
	Target map[string]string `json:"target"`
	Meta   ImportEntryMeta   `json:"meta"`
}

type ImportEntryMeta struct {
	Status  string `json:"status"`
	Comment string `json:"comment"`
}

type ExportDocumentPayload struct {
	Document string               `json:"document"`
	Entries  []ImportEntryPayload `json:"entries"`
}

type CreateGlossaryTermInput struct {
	SourceTerm     string
	TargetTerm     string
	SourceLanguage string
	TargetLanguage string
	Note           string
	CreatedBy      string
}

type UpdateGlossaryTermInput struct {
	SourceTerm     string
	TargetTerm     string
	SourceLanguage string
	TargetLanguage string
	Note           string
}

type CreateTMEntryInput struct {
	SourceLanguage string
	TargetLanguage string
	SourceText     string
	TargetText     string
	QualityScore   int
}

type UpdateTMEntryInput struct {
	SourceLanguage string
	TargetLanguage string
	SourceText     string
	TargetText     string
	QualityScore   int
}

type Organization struct {
	ID          string    `json:"id"`
	Slug        string    `json:"slug"`
	Name        string    `json:"name"`
	Description string    `json:"description"`
	Visibility  string    `json:"visibility"`
	CreatedAt   time.Time `json:"createdAt"`
	UpdatedAt   time.Time `json:"updatedAt"`
}

type Project struct {
	ID              string    `json:"id"`
	OrganizationID  string    `json:"organizationId"`
	Slug            string    `json:"slug"`
	Name            string    `json:"name"`
	Description     string    `json:"description"`
	TargetLanguage  string    `json:"targetLanguage"`
	SourceLanguages []string  `json:"sourceLanguages"`
	Visibility      string    `json:"visibility"`
	GuestPolicy     string    `json:"guestPolicy"`
	CreatedAt       time.Time `json:"createdAt"`
	UpdatedAt       time.Time `json:"updatedAt"`
}

type ProjectOverview struct {
	Project
	DocumentCount       int            `json:"documentCount"`
	TranslationUnitCount int           `json:"translationUnitCount"`
	StatusCounts        map[string]int `json:"statusCounts"`
}

type UserSummary struct {
	ID   string `json:"id"`
	Name string `json:"name"`
}

type DocumentTag struct {
	ID        string `json:"id"`
	Code      string `json:"code"`
	Name      string `json:"name"`
	Color     string `json:"color"`
	IsVisible bool   `json:"isVisible"`
}

type Document struct {
	ID               string        `json:"id"`
	ProjectID        string        `json:"projectId"`
	Path             string        `json:"path"`
	Title            string        `json:"title"`
	CurrentVersionNo int           `json:"currentVersionNo"`
	Tags             []DocumentTag `json:"tags"`
	UpdatedAt        time.Time     `json:"updatedAt"`
	UpdatedBy        *UserSummary  `json:"updatedBy,omitempty"`
}

type DocumentVersion struct {
	ID                 string       `json:"id"`
	DocumentID         string       `json:"documentId"`
	VersionNo          int          `json:"versionNo"`
	SourceSnapshotHash string       `json:"sourceSnapshotHash"`
	CreatedAt          time.Time    `json:"createdAt"`
	CreatedBy          *UserSummary `json:"createdBy,omitempty"`
}

type TranslationTarget struct {
	Language string `json:"language"`
	Text     string `json:"text"`
}

type UnitPermissions struct {
	CanView    bool `json:"canView"`
	CanEdit    bool `json:"canEdit"`
	CanReview  bool `json:"canReview"`
	CanApprove bool `json:"canApprove"`
}

type DocumentRef struct {
	ID   string        `json:"id"`
	Path string        `json:"path"`
	Tags []DocumentTag `json:"tags"`
}

type TranslationUnit struct {
	ID         string            `json:"id"`
	Key        string            `json:"key"`
	Document   DocumentRef       `json:"document"`
	Sources    map[string]string `json:"sources"`
	Target     TranslationTarget `json:"target"`
	Status     string            `json:"status"`
	Comment    string            `json:"comment"`
	Version    int               `json:"version"`
	UpdatedAt  time.Time         `json:"updatedAt"`
	UpdatedBy  *UserSummary      `json:"updatedBy,omitempty"`
	Permissions UnitPermissions  `json:"permissions"`
}

type TranslationRevision struct {
	ID               string       `json:"id"`
	RevisionNo       int          `json:"revisionNo"`
	BeforeTargetText string       `json:"beforeTargetText"`
	AfterTargetText  string       `json:"afterTargetText"`
	BeforeStatus     string       `json:"beforeStatus"`
	AfterStatus      string       `json:"afterStatus"`
	ChangeNote       string       `json:"changeNote"`
	ChangedAt        time.Time    `json:"changedAt"`
	ChangedBy        *UserSummary `json:"changedBy,omitempty"`
}

type DocumentListFilter struct {
	ProjectID string
	Path      string
	Tag       string
	UpdatedBy string
	Page      int
	PageSize  int
}

type UnitListFilter struct {
	ActorID      string
	ProjectID    string
	DocumentID   string
	DocumentPath string
	Tag          string
	Key          string
	Status       string
	SourceText   string
	TargetText   string
	UpdatedBy    string
	Page         int
	PageSize     int
}

func New(ctx context.Context, databaseURL string) (*Store, error) {
	pool, err := pgxpool.New(ctx, databaseURL)
	if err != nil {
		return nil, err
	}

	if err := pool.Ping(ctx); err != nil {
		pool.Close()
		return nil, err
	}

	return &Store{pool: pool}, nil
}

func (s *Store) Close() {
	if s.pool != nil {
		s.pool.Close()
	}
}

func (s *Store) ListOrganizations(ctx context.Context) ([]Organization, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT id::text, slug, name, description, visibility, created_at, updated_at
		FROM organizations
		ORDER BY name ASC
	`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var items []Organization
	for rows.Next() {
		var item Organization
		if err := rows.Scan(
			&item.ID,
			&item.Slug,
			&item.Name,
			&item.Description,
			&item.Visibility,
			&item.CreatedAt,
			&item.UpdatedAt,
		); err != nil {
			return nil, err
		}
		items = append(items, item)
	}

	return items, rows.Err()
}

func (s *Store) GetOrganization(ctx context.Context, organizationID string) (Organization, error) {
	var item Organization
	err := s.pool.QueryRow(ctx, `
		SELECT id::text, slug, name, description, visibility, created_at, updated_at
		FROM organizations
		WHERE id = $1
	`, organizationID).Scan(
		&item.ID,
		&item.Slug,
		&item.Name,
		&item.Description,
		&item.Visibility,
		&item.CreatedAt,
		&item.UpdatedAt,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return Organization{}, ErrNotFound
	}
	return item, err
}

func (s *Store) CreateOrganization(ctx context.Context, input CreateOrganizationInput) (Organization, error) {
	var item Organization
	err := s.pool.QueryRow(ctx, `
		INSERT INTO organizations (slug, name, description, visibility, created_by)
		VALUES ($1, $2, $3, $4, NULLIF($5, '')::uuid)
		RETURNING id::text, slug, name, description, visibility, created_at, updated_at
	`, input.Slug, input.Name, input.Description, input.Visibility, nullableUUID(input.CreatedBy)).Scan(
		&item.ID,
		&item.Slug,
		&item.Name,
		&item.Description,
		&item.Visibility,
		&item.CreatedAt,
		&item.UpdatedAt,
	)
	return item, err
}

func (s *Store) UpdateOrganization(ctx context.Context, organizationID string, input UpdateOrganizationInput) (Organization, error) {
	var item Organization
	err := s.pool.QueryRow(ctx, `
		UPDATE organizations
		SET name = $2,
			description = $3,
			visibility = $4,
			updated_at = NOW()
		WHERE id = $1
		RETURNING id::text, slug, name, description, visibility, created_at, updated_at
	`, organizationID, input.Name, input.Description, input.Visibility).Scan(
		&item.ID,
		&item.Slug,
		&item.Name,
		&item.Description,
		&item.Visibility,
		&item.CreatedAt,
		&item.UpdatedAt,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return Organization{}, ErrNotFound
	}
	return item, err
}

func (s *Store) ListProjectsByOrganization(ctx context.Context, organizationID string) ([]Project, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT
			p.id::text,
			p.organization_id::text,
			p.slug,
			p.name,
			p.description,
			p.target_language,
			COALESCE(
				array_agg(psl.language_code ORDER BY psl.sort_order) FILTER (WHERE psl.language_code IS NOT NULL),
				'{}'
			) AS source_languages,
			p.visibility,
			p.guest_policy,
			p.created_at,
			p.updated_at
		FROM projects p
		LEFT JOIN project_source_languages psl ON psl.project_id = p.id
		WHERE p.organization_id = $1
		GROUP BY p.id
		ORDER BY p.name ASC
	`, organizationID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var items []Project
	for rows.Next() {
		var item Project
		if err := rows.Scan(
			&item.ID,
			&item.OrganizationID,
			&item.Slug,
			&item.Name,
			&item.Description,
			&item.TargetLanguage,
			&item.SourceLanguages,
			&item.Visibility,
			&item.GuestPolicy,
			&item.CreatedAt,
			&item.UpdatedAt,
		); err != nil {
			return nil, err
		}
		items = append(items, item)
	}

	return items, rows.Err()
}

func (s *Store) GetProject(ctx context.Context, projectID string) (ProjectOverview, error) {
	var item ProjectOverview
	var statusCountsJSON []byte

	err := s.pool.QueryRow(ctx, `
		SELECT
			p.id::text,
			p.organization_id::text,
			p.slug,
			p.name,
			p.description,
			p.target_language,
			COALESCE(
				array_agg(psl.language_code ORDER BY psl.sort_order) FILTER (WHERE psl.language_code IS NOT NULL),
				'{}'
			) AS source_languages,
			p.visibility,
			p.guest_policy,
			p.created_at,
			p.updated_at,
			(SELECT COUNT(*) FROM documents d WHERE d.project_id = p.id) AS document_count,
			(SELECT COUNT(*) FROM translation_units tu WHERE tu.project_id = p.id) AS translation_unit_count,
			COALESCE((
				SELECT jsonb_object_agg(status_name, status_count)
				FROM (
					SELECT tu.status AS status_name, COUNT(*) AS status_count
					FROM translation_units tu
					WHERE tu.project_id = p.id
					GROUP BY tu.status
				) status_counts
			), '{}'::jsonb) AS status_counts
		FROM projects p
		LEFT JOIN project_source_languages psl ON psl.project_id = p.id
		WHERE p.id = $1
		GROUP BY p.id
	`, projectID).Scan(
		&item.ID,
		&item.OrganizationID,
		&item.Slug,
		&item.Name,
		&item.Description,
		&item.TargetLanguage,
		&item.SourceLanguages,
		&item.Visibility,
		&item.GuestPolicy,
		&item.CreatedAt,
		&item.UpdatedAt,
		&item.DocumentCount,
		&item.TranslationUnitCount,
		&statusCountsJSON,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return ProjectOverview{}, ErrNotFound
	}
	if err != nil {
		return ProjectOverview{}, err
	}

	item.StatusCounts = map[string]int{}
	if len(statusCountsJSON) > 0 {
		if err := json.Unmarshal(statusCountsJSON, &item.StatusCounts); err != nil {
			return ProjectOverview{}, err
		}
	}

	return item, nil
}

func (s *Store) CreateProject(ctx context.Context, organizationID string, input CreateProjectInput) (ProjectOverview, error) {
	tx, err := s.pool.BeginTx(ctx, pgx.TxOptions{})
	if err != nil {
		return ProjectOverview{}, err
	}
	defer tx.Rollback(ctx)

	var projectID string
	err = tx.QueryRow(ctx, `
		INSERT INTO projects (
			organization_id,
			slug,
			name,
			description,
			target_language,
			visibility,
			guest_policy,
			created_by
		)
		VALUES ($1, $2, $3, $4, $5, $6, $7, NULLIF($8, '')::uuid)
		RETURNING id::text
	`, organizationID, input.Slug, input.Name, input.Description, input.TargetLanguage, input.Visibility, input.GuestPolicy, nullableUUID(input.CreatedBy)).Scan(&projectID)
	if err != nil {
		return ProjectOverview{}, err
	}

	for index, languageCode := range uniqueNonEmptyStrings(input.SourceLanguages) {
		_, err := tx.Exec(ctx, `
			INSERT INTO project_source_languages (project_id, language_code, sort_order)
			VALUES ($1, $2, $3)
		`, projectID, languageCode, index+1)
		if err != nil {
			return ProjectOverview{}, err
		}
	}

	if err := tx.Commit(ctx); err != nil {
		return ProjectOverview{}, err
	}

	return s.GetProject(ctx, projectID)
}

func (s *Store) UpdateProject(ctx context.Context, projectID string, input UpdateProjectInput) (ProjectOverview, error) {
	tx, err := s.pool.BeginTx(ctx, pgx.TxOptions{})
	if err != nil {
		return ProjectOverview{}, err
	}
	defer tx.Rollback(ctx)

	commandTag, err := tx.Exec(ctx, `
		UPDATE projects
		SET name = $2,
			description = $3,
			target_language = $4,
			visibility = $5,
			guest_policy = $6,
			updated_at = NOW()
		WHERE id = $1
	`, projectID, input.Name, input.Description, input.TargetLanguage, input.Visibility, input.GuestPolicy)
	if err != nil {
		return ProjectOverview{}, err
	}
	if commandTag.RowsAffected() == 0 {
		return ProjectOverview{}, ErrNotFound
	}

	_, err = tx.Exec(ctx, `DELETE FROM project_source_languages WHERE project_id = $1`, projectID)
	if err != nil {
		return ProjectOverview{}, err
	}

	for index, languageCode := range uniqueNonEmptyStrings(input.SourceLanguages) {
		_, err := tx.Exec(ctx, `
			INSERT INTO project_source_languages (project_id, language_code, sort_order)
			VALUES ($1, $2, $3)
		`, projectID, languageCode, index+1)
		if err != nil {
			return ProjectOverview{}, err
		}
	}

	if err := tx.Commit(ctx); err != nil {
		return ProjectOverview{}, err
	}

	return s.GetProject(ctx, projectID)
}

func (s *Store) ListTags(ctx context.Context, projectID string) ([]DocumentTag, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT id::text, code, name, color, is_visible
		FROM document_tags
		WHERE project_id = $1
		ORDER BY name ASC
	`, projectID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var items []DocumentTag
	for rows.Next() {
		var item DocumentTag
		if err := rows.Scan(&item.ID, &item.Code, &item.Name, &item.Color, &item.IsVisible); err != nil {
			return nil, err
		}
		items = append(items, item)
	}

	return items, rows.Err()
}

func (s *Store) ListDocuments(ctx context.Context, filter DocumentListFilter) ([]Document, int, error) {
	var args []any
	var where []string

	args = append(args, filter.ProjectID)
	where = append(where, fmt.Sprintf("d.project_id = $%d", len(args)))

	if filter.Path != "" {
		args = append(args, "%"+filter.Path+"%")
		where = append(where, fmt.Sprintf("d.path ILIKE $%d", len(args)))
	}
	if filter.Tag != "" {
		args = append(args, filter.Tag)
		where = append(where, fmt.Sprintf(`
			EXISTS (
				SELECT 1
				FROM document_tag_bindings dtbx
				JOIN document_tags dtx ON dtx.id = dtbx.tag_id
				WHERE dtbx.document_id = d.id
				  AND (dtx.code = $%d OR dtx.name = $%d)
			)
		`, len(args), len(args)))
	}
	if filter.UpdatedBy != "" {
		args = append(args, filter.UpdatedBy)
		where = append(where, fmt.Sprintf("d.updated_by::text = $%d", len(args)))
	}

	baseWhere := "WHERE " + strings.Join(where, " AND ")

	var total int
	countQuery := "SELECT COUNT(*) FROM documents d " + baseWhere
	if err := s.pool.QueryRow(ctx, countQuery, args...).Scan(&total); err != nil {
		return nil, 0, err
	}

	args = append(args, filter.PageSize, (filter.Page-1)*filter.PageSize)
	query := `
		SELECT
			d.id::text,
			d.project_id::text,
			d.path,
			d.title,
			d.current_version_no,
			d.updated_at,
			u.id::text,
			u.display_name,
			COALESCE((
				SELECT jsonb_agg(jsonb_build_object(
					'id', t.id::text,
					'code', t.code,
					'name', t.name,
					'color', t.color,
					'isVisible', t.is_visible
				) ORDER BY t.name ASC)
				FROM document_tag_bindings dtb
				JOIN document_tags t ON t.id = dtb.tag_id
				WHERE dtb.document_id = d.id
			), '[]'::jsonb) AS tags
		FROM documents d
		LEFT JOIN users u ON u.id = d.updated_by
	` + baseWhere + `
		ORDER BY d.path ASC
		LIMIT $` + fmt.Sprintf("%d", len(args)-1) + ` OFFSET $` + fmt.Sprintf("%d", len(args))

	rows, err := s.pool.Query(ctx, query, args...)
	if err != nil {
		return nil, 0, err
	}
	defer rows.Close()

	var items []Document
	for rows.Next() {
		var item Document
		var userID *string
		var userName *string
		var tagsJSON []byte
		if err := rows.Scan(
			&item.ID,
			&item.ProjectID,
			&item.Path,
			&item.Title,
			&item.CurrentVersionNo,
			&item.UpdatedAt,
			&userID,
			&userName,
			&tagsJSON,
		); err != nil {
			return nil, 0, err
		}
		if err := json.Unmarshal(tagsJSON, &item.Tags); err != nil {
			return nil, 0, err
		}
		item.UpdatedBy = userSummaryFromPointers(userID, userName)
		items = append(items, item)
	}

	return items, total, rows.Err()
}

func (s *Store) GetDocument(ctx context.Context, projectID, documentID string) (Document, error) {
	var item Document
	var userID *string
	var userName *string
	var tagsJSON []byte

	err := s.pool.QueryRow(ctx, `
		SELECT
			d.id::text,
			d.project_id::text,
			d.path,
			d.title,
			d.current_version_no,
			d.updated_at,
			u.id::text,
			u.display_name,
			COALESCE((
				SELECT jsonb_agg(jsonb_build_object(
					'id', t.id::text,
					'code', t.code,
					'name', t.name,
					'color', t.color,
					'isVisible', t.is_visible
				) ORDER BY t.name ASC)
				FROM document_tag_bindings dtb
				JOIN document_tags t ON t.id = dtb.tag_id
				WHERE dtb.document_id = d.id
			), '[]'::jsonb) AS tags
		FROM documents d
		LEFT JOIN users u ON u.id = d.updated_by
		WHERE d.project_id = $1 AND d.id = $2
	`, projectID, documentID).Scan(
		&item.ID,
		&item.ProjectID,
		&item.Path,
		&item.Title,
		&item.CurrentVersionNo,
		&item.UpdatedAt,
		&userID,
		&userName,
		&tagsJSON,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return Document{}, ErrNotFound
	}
	if err != nil {
		return Document{}, err
	}
	if err := json.Unmarshal(tagsJSON, &item.Tags); err != nil {
		return Document{}, err
	}
	item.UpdatedBy = userSummaryFromPointers(userID, userName)
	return item, nil
}

func (s *Store) CreateDocument(ctx context.Context, projectID string, input CreateDocumentInput) (Document, error) {
	var item Document
	var userID *string
	var userName *string
	var tagsJSON []byte

	err := s.pool.QueryRow(ctx, `
		INSERT INTO documents (project_id, path, title, updated_by)
		VALUES ($1, $2, $3, NULLIF($4, '')::uuid)
		RETURNING
			id::text,
			project_id::text,
			path,
			title,
			current_version_no,
			updated_at,
			NULLIF($4, '')::text,
			(SELECT display_name FROM users WHERE id = NULLIF($4, '')::uuid),
			'[]'::jsonb
	`, projectID, input.Path, input.Title, nullableUUID(input.UpdatedBy)).Scan(
		&item.ID,
		&item.ProjectID,
		&item.Path,
		&item.Title,
		&item.CurrentVersionNo,
		&item.UpdatedAt,
		&userID,
		&userName,
		&tagsJSON,
	)
	if err != nil {
		return Document{}, err
	}
	if err := json.Unmarshal(tagsJSON, &item.Tags); err != nil {
		return Document{}, err
	}
	item.UpdatedBy = userSummaryFromPointers(userID, userName)
	return item, nil
}

func (s *Store) UpdateDocument(ctx context.Context, projectID, documentID string, input UpdateDocumentInput) (Document, error) {
	var item Document
	var userID *string
	var userName *string
	var tagsJSON []byte

	err := s.pool.QueryRow(ctx, `
		UPDATE documents
		SET title = $3,
			updated_at = NOW(),
			updated_by = NULLIF($4, '')::uuid
		WHERE project_id = $1 AND id = $2
		RETURNING
			id::text,
			project_id::text,
			path,
			title,
			current_version_no,
			updated_at,
			NULLIF($4, '')::text,
			(SELECT display_name FROM users WHERE id = NULLIF($4, '')::uuid),
			COALESCE((
				SELECT jsonb_agg(jsonb_build_object(
					'id', t.id::text,
					'code', t.code,
					'name', t.name,
					'color', t.color,
					'isVisible', t.is_visible
				) ORDER BY t.name ASC)
				FROM document_tag_bindings dtb
				JOIN document_tags t ON t.id = dtb.tag_id
				WHERE dtb.document_id = documents.id
			), '[]'::jsonb)
	`, projectID, documentID, input.Title, nullableUUID(input.UpdatedBy)).Scan(
		&item.ID,
		&item.ProjectID,
		&item.Path,
		&item.Title,
		&item.CurrentVersionNo,
		&item.UpdatedAt,
		&userID,
		&userName,
		&tagsJSON,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return Document{}, ErrNotFound
	}
	if err != nil {
		return Document{}, err
	}
	if err := json.Unmarshal(tagsJSON, &item.Tags); err != nil {
		return Document{}, err
	}
	item.UpdatedBy = userSummaryFromPointers(userID, userName)
	return item, nil
}

func (s *Store) ListDocumentVersions(ctx context.Context, projectID, documentID string) ([]DocumentVersion, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT
			dv.id::text,
			dv.document_id::text,
			dv.version_no,
			dv.source_snapshot_hash,
			dv.created_at,
			u.id::text,
			u.display_name
		FROM document_versions dv
		JOIN documents d ON d.id = dv.document_id
		LEFT JOIN users u ON u.id = dv.created_by
		WHERE d.project_id = $1 AND dv.document_id = $2
		ORDER BY dv.version_no DESC
	`, projectID, documentID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var items []DocumentVersion
	for rows.Next() {
		var item DocumentVersion
		var userID *string
		var userName *string
		if err := rows.Scan(
			&item.ID,
			&item.DocumentID,
			&item.VersionNo,
			&item.SourceSnapshotHash,
			&item.CreatedAt,
			&userID,
			&userName,
		); err != nil {
			return nil, err
		}
		item.CreatedBy = userSummaryFromPointers(userID, userName)
		items = append(items, item)
	}

	return items, rows.Err()
}

func (s *Store) ListTranslationUnits(ctx context.Context, filter UnitListFilter) ([]TranslationUnit, int, error) {
	var args []any
	var where []string

	args = append(args, filter.ProjectID)
	where = append(where, fmt.Sprintf("tu.project_id = $%d", len(args)))

	if filter.DocumentID != "" {
		args = append(args, filter.DocumentID)
		where = append(where, fmt.Sprintf("tu.document_id = $%d", len(args)))
	}
	if filter.DocumentPath != "" {
		args = append(args, "%"+filter.DocumentPath+"%")
		where = append(where, fmt.Sprintf("d.path ILIKE $%d", len(args)))
	}
	if filter.Tag != "" {
		args = append(args, filter.Tag)
		where = append(where, fmt.Sprintf(`
			EXISTS (
				SELECT 1
				FROM document_tag_bindings dtbx
				JOIN document_tags dtx ON dtx.id = dtbx.tag_id
				WHERE dtbx.document_id = d.id
				  AND (dtx.code = $%d OR dtx.name = $%d)
			)
		`, len(args), len(args)))
	}
	if filter.Key != "" {
		args = append(args, "%"+filter.Key+"%")
		where = append(where, fmt.Sprintf("tu.key ILIKE $%d", len(args)))
	}
	if filter.Status != "" {
		args = append(args, filter.Status)
		where = append(where, fmt.Sprintf("tu.status = $%d", len(args)))
	}
	if filter.SourceText != "" {
		args = append(args, "%"+filter.SourceText+"%")
		where = append(where, fmt.Sprintf(`
			EXISTS (
				SELECT 1
				FROM translation_unit_sources tusx
				WHERE tusx.translation_unit_id = tu.id
				  AND tusx.text ILIKE $%d
			)
		`, len(args)))
	}
	if filter.TargetText != "" {
		args = append(args, "%"+filter.TargetText+"%")
		where = append(where, fmt.Sprintf("tu.target_text ILIKE $%d", len(args)))
	}
	if filter.UpdatedBy != "" {
		args = append(args, filter.UpdatedBy)
		where = append(where, fmt.Sprintf("tu.updated_by::text = $%d", len(args)))
	}

	baseWhere := "WHERE " + strings.Join(where, " AND ")

	var total int
	countQuery := `
		SELECT COUNT(*)
		FROM translation_units tu
		JOIN documents d ON d.id = tu.document_id
	` + baseWhere
	if err := s.pool.QueryRow(ctx, countQuery, args...).Scan(&total); err != nil {
		return nil, 0, err
	}

	args = append(args, filter.PageSize, (filter.Page-1)*filter.PageSize)
	query := `
		SELECT
			tu.id::text,
			tu.key,
			d.id::text,
			d.path,
			COALESCE((
				SELECT jsonb_agg(jsonb_build_object(
					'id', t.id::text,
					'code', t.code,
					'name', t.name,
					'color', t.color,
					'isVisible', t.is_visible
				) ORDER BY t.name ASC)
				FROM document_tag_bindings dtb
				JOIN document_tags t ON t.id = dtb.tag_id
				WHERE dtb.document_id = d.id
			), '[]'::jsonb) AS tags,
			COALESCE((
				SELECT jsonb_object_agg(tus.language_code, tus.text)
				FROM translation_unit_sources tus
				WHERE tus.translation_unit_id = tu.id
			), '{}'::jsonb) AS sources,
			tu.target_language,
			tu.target_text,
			tu.status,
			tu.comment,
			tu.version_no,
			tu.updated_at,
			u.id::text,
			u.display_name
		FROM translation_units tu
		JOIN documents d ON d.id = tu.document_id
		LEFT JOIN users u ON u.id = tu.updated_by
	` + baseWhere + `
		ORDER BY d.path ASC, tu.key ASC
		LIMIT $` + fmt.Sprintf("%d", len(args)-1) + ` OFFSET $` + fmt.Sprintf("%d", len(args))

	rows, err := s.pool.Query(ctx, query, args...)
	if err != nil {
		return nil, 0, err
	}
	defer rows.Close()

	var items []TranslationUnit
	for rows.Next() {
		item, err := scanTranslationUnit(rows)
		if err != nil {
			return nil, 0, err
		}
		items = append(items, item)
	}
	if err := rows.Err(); err != nil {
		return nil, 0, err
	}

	tx, err := s.pool.BeginTx(ctx, pgx.TxOptions{})
	if err != nil {
		return nil, 0, err
	}
	defer tx.Rollback(ctx)

	for index := range items {
		permissions, err := s.resolveUnitPermissions(ctx, tx, filter.ProjectID, filter.ActorID, items[index])
		if err != nil {
			return nil, 0, err
		}
		items[index].Permissions = permissions
	}

	if err := tx.Commit(ctx); err != nil {
		return nil, 0, err
	}

	return items, total, nil
}

func (s *Store) GetTranslationUnit(ctx context.Context, projectID, unitID, actorID string) (TranslationUnit, error) {
	query := `
		SELECT
			tu.id::text,
			tu.key,
			d.id::text,
			d.path,
			COALESCE((
				SELECT jsonb_agg(jsonb_build_object(
					'id', t.id::text,
					'code', t.code,
					'name', t.name,
					'color', t.color,
					'isVisible', t.is_visible
				) ORDER BY t.name ASC)
				FROM document_tag_bindings dtb
				JOIN document_tags t ON t.id = dtb.tag_id
				WHERE dtb.document_id = d.id
			), '[]'::jsonb) AS tags,
			COALESCE((
				SELECT jsonb_object_agg(tus.language_code, tus.text)
				FROM translation_unit_sources tus
				WHERE tus.translation_unit_id = tu.id
			), '{}'::jsonb) AS sources,
			tu.target_language,
			tu.target_text,
			tu.status,
			tu.comment,
			tu.version_no,
			tu.updated_at,
			u.id::text,
			u.display_name
		FROM translation_units tu
		JOIN documents d ON d.id = tu.document_id
		LEFT JOIN users u ON u.id = tu.updated_by
		WHERE tu.project_id = $1 AND tu.id = $2
	`
	row := s.pool.QueryRow(ctx, query, projectID, unitID)
	item, err := scanTranslationUnit(row)
	if errors.Is(err, pgx.ErrNoRows) {
		return TranslationUnit{}, ErrNotFound
	}
	if err != nil {
		return TranslationUnit{}, err
	}

	tx, err := s.pool.BeginTx(ctx, pgx.TxOptions{})
	if err != nil {
		return TranslationUnit{}, err
	}
	defer tx.Rollback(ctx)

	permissions, err := s.resolveUnitPermissions(ctx, tx, projectID, actorID, item)
	if err != nil {
		return TranslationUnit{}, err
	}
	item.Permissions = permissions

	if err := tx.Commit(ctx); err != nil {
		return TranslationUnit{}, err
	}

	return item, nil
}

func (s *Store) ListTranslationUnitHistory(ctx context.Context, projectID, unitID string) ([]TranslationRevision, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT
			tr.id::text,
			tr.revision_no,
			tr.before_target_text,
			tr.after_target_text,
			tr.before_status,
			tr.after_status,
			tr.change_note,
			tr.changed_at,
			u.id::text,
			u.display_name
		FROM translation_revisions tr
		JOIN translation_units tu ON tu.id = tr.translation_unit_id
		LEFT JOIN users u ON u.id = tr.changed_by
		WHERE tu.project_id = $1 AND tr.translation_unit_id = $2
		ORDER BY tr.revision_no DESC
	`, projectID, unitID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var items []TranslationRevision
	for rows.Next() {
		var item TranslationRevision
		var userID *string
		var userName *string
		if err := rows.Scan(
			&item.ID,
			&item.RevisionNo,
			&item.BeforeTargetText,
			&item.AfterTargetText,
			&item.BeforeStatus,
			&item.AfterStatus,
			&item.ChangeNote,
			&item.ChangedAt,
			&userID,
			&userName,
		); err != nil {
			return nil, err
		}
		item.ChangedBy = userSummaryFromPointers(userID, userName)
		items = append(items, item)
	}

	return items, rows.Err()
}

func (s *Store) ListProjectMembers(ctx context.Context, projectID string) ([]ProjectMember, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT
			pm.id::text,
			u.id::text,
			u.display_name,
			pm.role_code,
			pm.created_at
		FROM project_members pm
		JOIN users u ON u.id = pm.user_id
		WHERE pm.project_id = $1
		ORDER BY u.display_name ASC
	`, projectID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var items []ProjectMember
	for rows.Next() {
		var item ProjectMember
		if err := rows.Scan(
			&item.ID,
			&item.User.ID,
			&item.User.Name,
			&item.RoleCode,
			&item.CreatedAt,
		); err != nil {
			return nil, err
		}
		items = append(items, item)
	}
	return items, rows.Err()
}

func (s *Store) UpsertProjectMember(ctx context.Context, projectID string, input UpsertProjectMemberInput) error {
	_, err := s.pool.Exec(ctx, `
		INSERT INTO project_members (project_id, user_id, role_code)
		VALUES ($1, $2::uuid, $3)
		ON CONFLICT (project_id, user_id)
		DO UPDATE SET role_code = EXCLUDED.role_code
	`, projectID, input.UserID, input.RoleCode)
	return err
}

func (s *Store) DeleteProjectMember(ctx context.Context, projectID, userID string) error {
	commandTag, err := s.pool.Exec(ctx, `
		DELETE FROM project_members
		WHERE project_id = $1 AND user_id = $2::uuid
	`, projectID, userID)
	if err != nil {
		return err
	}
	if commandTag.RowsAffected() == 0 {
		return ErrNotFound
	}
	return nil
}

func (s *Store) ListProjectRoles(ctx context.Context, projectID string) ([]ProjectRole, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT
			pr.id::text,
			pr.code,
			pr.name,
			pr.is_system,
			COALESCE((
				SELECT jsonb_agg(jsonb_build_object(
					'id', pn.id::text,
					'code', pn.code,
					'name', pn.name,
					'description', pn.description
				) ORDER BY pn.code ASC)
				FROM project_role_permissions prp
				JOIN permission_nodes pn ON pn.id = prp.permission_node_id
				WHERE prp.project_role_id = pr.id
			), '[]'::jsonb) AS permissions
		FROM project_roles pr
		WHERE pr.project_id = $1
		ORDER BY pr.is_system DESC, pr.code ASC
	`, projectID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var items []ProjectRole
	for rows.Next() {
		var item ProjectRole
		var permissionsJSON []byte
		if err := rows.Scan(
			&item.ID,
			&item.Code,
			&item.Name,
			&item.IsSystem,
			&permissionsJSON,
		); err != nil {
			return nil, err
		}
		if err := json.Unmarshal(permissionsJSON, &item.Permissions); err != nil {
			return nil, err
		}
		items = append(items, item)
	}
	return items, rows.Err()
}

func (s *Store) ListPermissionNodes(ctx context.Context) ([]PermissionNode, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT id::text, code, name, description
		FROM permission_nodes
		ORDER BY code ASC
	`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var items []PermissionNode
	for rows.Next() {
		var item PermissionNode
		if err := rows.Scan(&item.ID, &item.Code, &item.Name, &item.Description); err != nil {
			return nil, err
		}
		items = append(items, item)
	}
	return items, rows.Err()
}

func (s *Store) ListPermissionOverrides(ctx context.Context, projectID string) ([]PermissionOverride, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT
			po.id::text,
			u.id::text,
			u.display_name,
			pn.id::text,
			pn.code,
			pn.name,
			pn.description,
			po.effect
		FROM project_member_permission_overrides po
		JOIN users u ON u.id = po.user_id
		JOIN permission_nodes pn ON pn.id = po.permission_node_id
		WHERE po.project_id = $1
		ORDER BY u.display_name ASC, pn.code ASC
	`, projectID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var items []PermissionOverride
	for rows.Next() {
		var item PermissionOverride
		if err := rows.Scan(
			&item.ID,
			&item.User.ID,
			&item.User.Name,
			&item.PermissionNode.ID,
			&item.PermissionNode.Code,
			&item.PermissionNode.Name,
			&item.PermissionNode.Description,
			&item.Effect,
		); err != nil {
			return nil, err
		}
		items = append(items, item)
	}
	return items, rows.Err()
}

func (s *Store) ListDocumentRules(ctx context.Context, projectID string) ([]DocumentRule, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT
			dr.id::text,
			u.id::text,
			u.display_name,
			dr.permission_scope,
			dr.match_type,
			dr.match_value,
			dr.effect,
			dr.created_at
		FROM project_member_document_rules dr
		JOIN users u ON u.id = dr.user_id
		WHERE dr.project_id = $1
		ORDER BY u.display_name ASC, dr.permission_scope ASC, dr.match_type ASC
	`, projectID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var items []DocumentRule
	for rows.Next() {
		var item DocumentRule
		if err := rows.Scan(
			&item.ID,
			&item.User.ID,
			&item.User.Name,
			&item.PermissionScope,
			&item.MatchType,
			&item.MatchValue,
			&item.Effect,
			&item.CreatedAt,
		); err != nil {
			return nil, err
		}
		items = append(items, item)
	}
	return items, rows.Err()
}

func (s *Store) SetPermissionOverride(ctx context.Context, projectID string, input SetPermissionOverrideInput) error {
	_, err := s.pool.Exec(ctx, `
		INSERT INTO project_member_permission_overrides (project_id, user_id, permission_node_id, effect)
		VALUES ($1, $2::uuid, $3::uuid, $4)
		ON CONFLICT (project_id, user_id, permission_node_id)
		DO UPDATE SET effect = EXCLUDED.effect
	`, projectID, input.UserID, input.PermissionNodeID, input.Effect)
	return err
}

func (s *Store) DeletePermissionOverride(ctx context.Context, projectID, userID, permissionNodeID string) error {
	commandTag, err := s.pool.Exec(ctx, `
		DELETE FROM project_member_permission_overrides
		WHERE project_id = $1
		  AND user_id = $2::uuid
		  AND permission_node_id = $3::uuid
	`, projectID, userID, permissionNodeID)
	if err != nil {
		return err
	}
	if commandTag.RowsAffected() == 0 {
		return ErrNotFound
	}
	return nil
}

func (s *Store) SetDocumentRule(ctx context.Context, projectID string, input SetDocumentRuleInput) error {
	_, err := s.pool.Exec(ctx, `
		INSERT INTO project_member_document_rules (project_id, user_id, permission_scope, match_type, match_value, effect)
		VALUES ($1, $2::uuid, $3, $4, $5, $6)
	`, projectID, input.UserID, input.PermissionScope, input.MatchType, input.MatchValue, input.Effect)
	return err
}

func (s *Store) DeleteDocumentRule(ctx context.Context, projectID, ruleID string) error {
	commandTag, err := s.pool.Exec(ctx, `
		DELETE FROM project_member_document_rules
		WHERE project_id = $1 AND id = $2::uuid
	`, projectID, ruleID)
	if err != nil {
		return err
	}
	if commandTag.RowsAffected() == 0 {
		return ErrNotFound
	}
	return nil
}

func (s *Store) Login(ctx context.Context, email, password string) (string, AuthUser, error) {
	var user AuthUser
	var passwordHash string
	err := s.pool.QueryRow(ctx, `
		SELECT id::text, email, username, password_hash, display_name, preferred_locale, COALESCE(preferred_source_language, '')
		FROM users
		WHERE email = $1
	`, email).Scan(
		&user.ID,
		&user.Email,
		&user.Username,
		&passwordHash,
		&user.DisplayName,
		&user.PreferredLocale,
		&user.PreferredSourceLanguage,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return "", AuthUser{}, ErrNotFound
	}
	if err != nil {
		return "", AuthUser{}, err
	}
	if err := bcrypt.CompareHashAndPassword([]byte(passwordHash), []byte(password)); err != nil {
		return "", AuthUser{}, errors.New("invalid credentials")
	}

	token, err := randomToken()
	if err != nil {
		return "", AuthUser{}, err
	}

	_, err = s.pool.Exec(ctx, `
		INSERT INTO user_sessions (user_id, token, expires_at)
		VALUES ($1::uuid, $2, NOW() + INTERVAL '7 days')
	`, user.ID, token)
	if err != nil {
		return "", AuthUser{}, err
	}

	return token, user, nil
}

func (s *Store) Logout(ctx context.Context, token string) error {
	_, err := s.pool.Exec(ctx, `
		DELETE FROM user_sessions
		WHERE token = $1
	`, token)
	return err
}

func (s *Store) GetUserByToken(ctx context.Context, token string) (AuthUser, error) {
	var user AuthUser
	err := s.pool.QueryRow(ctx, `
		SELECT
			u.id::text,
			u.email,
			u.username,
			u.display_name,
			u.preferred_locale,
			COALESCE(u.preferred_source_language, '')
		FROM user_sessions us
		JOIN users u ON u.id = us.user_id
		WHERE us.token = $1
		  AND us.expires_at > NOW()
	`, token).Scan(
		&user.ID,
		&user.Email,
		&user.Username,
		&user.DisplayName,
		&user.PreferredLocale,
		&user.PreferredSourceLanguage,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return AuthUser{}, ErrNotFound
	}
	return user, err
}

func (s *Store) UpdateUserPreferences(ctx context.Context, userID, preferredLocale, preferredSourceLanguage string) (AuthUser, error) {
	var user AuthUser
	err := s.pool.QueryRow(ctx, `
		UPDATE users
		SET preferred_locale = $2,
			preferred_source_language = NULLIF($3, ''),
			updated_at = NOW()
		WHERE id = $1::uuid
		RETURNING id::text, email, username, display_name, preferred_locale, COALESCE(preferred_source_language, '')
	`, userID, preferredLocale, preferredSourceLanguage).Scan(
		&user.ID,
		&user.Email,
		&user.Username,
		&user.DisplayName,
		&user.PreferredLocale,
		&user.PreferredSourceLanguage,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return AuthUser{}, ErrNotFound
	}
	return user, err
}

func (s *Store) ListGlossaryTerms(ctx context.Context, projectID string) ([]GlossaryTerm, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT
			gt.id::text,
			gt.source_term,
			gt.target_term,
			gt.source_language,
			gt.target_language,
			gt.note,
			gt.created_at,
			u.id::text,
			u.display_name
		FROM glossary_terms gt
		LEFT JOIN users u ON u.id = gt.created_by
		WHERE gt.project_id = $1
		ORDER BY gt.source_term ASC
	`, projectID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var items []GlossaryTerm
	for rows.Next() {
		var item GlossaryTerm
		var userID *string
		var userName *string
		if err := rows.Scan(
			&item.ID,
			&item.SourceTerm,
			&item.TargetTerm,
			&item.SourceLanguage,
			&item.TargetLanguage,
			&item.Note,
			&item.CreatedAt,
			&userID,
			&userName,
		); err != nil {
			return nil, err
		}
		item.CreatedBy = userSummaryFromPointers(userID, userName)
		items = append(items, item)
	}
	return items, rows.Err()
}

func (s *Store) CreateGlossaryTerm(ctx context.Context, projectID string, input CreateGlossaryTermInput) (GlossaryTerm, error) {
	var item GlossaryTerm
	var userID *string
	var userName *string
	err := s.pool.QueryRow(ctx, `
		INSERT INTO glossary_terms (
			project_id,
			source_term,
			target_term,
			source_language,
			target_language,
			note,
			created_by
		)
		VALUES ($1, $2, $3, $4, $5, $6, NULLIF($7, '')::uuid)
		RETURNING
			id::text,
			source_term,
			target_term,
			source_language,
			target_language,
			note,
			created_at,
			NULLIF($7, '')::text,
			(SELECT display_name FROM users WHERE id = NULLIF($7, '')::uuid)
	`, projectID, input.SourceTerm, input.TargetTerm, input.SourceLanguage, input.TargetLanguage, input.Note, nullableUUID(input.CreatedBy)).Scan(
		&item.ID,
		&item.SourceTerm,
		&item.TargetTerm,
		&item.SourceLanguage,
		&item.TargetLanguage,
		&item.Note,
		&item.CreatedAt,
		&userID,
		&userName,
	)
	if err != nil {
		return GlossaryTerm{}, err
	}
	item.CreatedBy = userSummaryFromPointers(userID, userName)
	return item, nil
}

func (s *Store) UpdateGlossaryTerm(ctx context.Context, projectID, termID string, input UpdateGlossaryTermInput) (GlossaryTerm, error) {
	var item GlossaryTerm
	var userID *string
	var userName *string
	err := s.pool.QueryRow(ctx, `
		UPDATE glossary_terms
		SET source_term = $3,
			target_term = $4,
			source_language = $5,
			target_language = $6,
			note = $7,
			updated_at = NOW()
		WHERE project_id = $1 AND id = $2
		RETURNING
			id::text,
			source_term,
			target_term,
			source_language,
			target_language,
			note,
			created_at,
			created_by::text,
			(SELECT display_name FROM users WHERE id = glossary_terms.created_by)
	`, projectID, termID, input.SourceTerm, input.TargetTerm, input.SourceLanguage, input.TargetLanguage, input.Note).Scan(
		&item.ID,
		&item.SourceTerm,
		&item.TargetTerm,
		&item.SourceLanguage,
		&item.TargetLanguage,
		&item.Note,
		&item.CreatedAt,
		&userID,
		&userName,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return GlossaryTerm{}, ErrNotFound
	}
	if err != nil {
		return GlossaryTerm{}, err
	}
	item.CreatedBy = userSummaryFromPointers(userID, userName)
	return item, nil
}

func (s *Store) DeleteGlossaryTerm(ctx context.Context, projectID, termID string) error {
	commandTag, err := s.pool.Exec(ctx, `
		DELETE FROM glossary_terms
		WHERE project_id = $1 AND id = $2::uuid
	`, projectID, termID)
	if err != nil {
		return err
	}
	if commandTag.RowsAffected() == 0 {
		return ErrNotFound
	}
	return nil
}

func (s *Store) ListTMEntries(ctx context.Context, projectID string) ([]TranslationMemoryEntry, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT
			id::text,
			source_language,
			target_language,
			source_text,
			target_text,
			quality_score,
			created_from_unit_id::text,
			created_at
		FROM translation_memory_entries
		WHERE project_id = $1
		ORDER BY created_at DESC
	`, projectID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var items []TranslationMemoryEntry
	for rows.Next() {
		var item TranslationMemoryEntry
		if err := rows.Scan(
			&item.ID,
			&item.SourceLanguage,
			&item.TargetLanguage,
			&item.SourceText,
			&item.TargetText,
			&item.QualityScore,
			&item.CreatedFromUnitID,
			&item.CreatedAt,
		); err != nil {
			return nil, err
		}
		items = append(items, item)
	}
	return items, rows.Err()
}

func (s *Store) CreateTMEntry(ctx context.Context, projectID string, input CreateTMEntryInput) (TranslationMemoryEntry, error) {
	var item TranslationMemoryEntry
	err := s.pool.QueryRow(ctx, `
		INSERT INTO translation_memory_entries (
			project_id,
			source_language,
			target_language,
			source_text,
			target_text,
			quality_score
		)
		VALUES ($1, $2, $3, $4, $5, $6)
		RETURNING id::text, source_language, target_language, source_text, target_text, quality_score, created_from_unit_id::text, created_at
	`, projectID, input.SourceLanguage, input.TargetLanguage, input.SourceText, input.TargetText, input.QualityScore).Scan(
		&item.ID,
		&item.SourceLanguage,
		&item.TargetLanguage,
		&item.SourceText,
		&item.TargetText,
		&item.QualityScore,
		&item.CreatedFromUnitID,
		&item.CreatedAt,
	)
	return item, err
}

func (s *Store) UpdateTMEntry(ctx context.Context, projectID, entryID string, input UpdateTMEntryInput) (TranslationMemoryEntry, error) {
	var item TranslationMemoryEntry
	err := s.pool.QueryRow(ctx, `
		UPDATE translation_memory_entries
		SET source_language = $3,
			target_language = $4,
			source_text = $5,
			target_text = $6,
			quality_score = $7
		WHERE project_id = $1 AND id = $2
		RETURNING id::text, source_language, target_language, source_text, target_text, quality_score, created_from_unit_id::text, created_at
	`, projectID, entryID, input.SourceLanguage, input.TargetLanguage, input.SourceText, input.TargetText, input.QualityScore).Scan(
		&item.ID,
		&item.SourceLanguage,
		&item.TargetLanguage,
		&item.SourceText,
		&item.TargetText,
		&item.QualityScore,
		&item.CreatedFromUnitID,
		&item.CreatedAt,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return TranslationMemoryEntry{}, ErrNotFound
	}
	return item, err
}

func (s *Store) DeleteTMEntry(ctx context.Context, projectID, entryID string) error {
	commandTag, err := s.pool.Exec(ctx, `
		DELETE FROM translation_memory_entries
		WHERE project_id = $1 AND id = $2::uuid
	`, projectID, entryID)
	if err != nil {
		return err
	}
	if commandTag.RowsAffected() == 0 {
		return ErrNotFound
	}
	return nil
}

func (s *Store) ListImportJobs(ctx context.Context, projectID string) ([]ImportJob, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT
			ij.id::text,
			ij.document_path,
			ij.status,
			ij.error_message,
			ij.created_at,
			ij.started_at,
			ij.finished_at,
			u.id::text,
			u.display_name
		FROM import_jobs ij
		LEFT JOIN users u ON u.id = ij.uploaded_by
		WHERE ij.project_id = $1
		ORDER BY ij.created_at DESC
	`, projectID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var items []ImportJob
	for rows.Next() {
		var item ImportJob
		var userID *string
		var userName *string
		if err := rows.Scan(
			&item.ID,
			&item.DocumentPath,
			&item.Status,
			&item.ErrorMessage,
			&item.CreatedAt,
			&item.StartedAt,
			&item.FinishedAt,
			&userID,
			&userName,
		); err != nil {
			return nil, err
		}
		item.UploadedBy = userSummaryFromPointers(userID, userName)
		items = append(items, item)
	}
	return items, rows.Err()
}

func (s *Store) CreateImportJob(ctx context.Context, projectID, documentPath, userID string) (ImportJob, error) {
	var item ImportJob
	var createdByID *string
	var createdByName *string
	err := s.pool.QueryRow(ctx, `
		INSERT INTO import_jobs (project_id, document_path, status, uploaded_by, started_at, finished_at)
		VALUES ($1, $2, 'finished', NULLIF($3, '')::uuid, NOW(), NOW())
		RETURNING id::text, document_path, status, error_message, created_at, started_at, finished_at, NULLIF($3, '')::text, (SELECT display_name FROM users WHERE id = NULLIF($3, '')::uuid)
	`, projectID, documentPath, nullableUUID(userID)).Scan(
		&item.ID,
		&item.DocumentPath,
		&item.Status,
		&item.ErrorMessage,
		&item.CreatedAt,
		&item.StartedAt,
		&item.FinishedAt,
		&createdByID,
		&createdByName,
	)
	if err != nil {
		return ImportJob{}, err
	}
	item.UploadedBy = userSummaryFromPointers(createdByID, createdByName)
	return item, nil
}

func (s *Store) ImportDocument(ctx context.Context, projectID, userID string, payload ImportDocumentPayload) (ImportJob, error) {
	tx, err := s.pool.BeginTx(ctx, pgx.TxOptions{})
	if err != nil {
		return ImportJob{}, err
	}
	defer tx.Rollback(ctx)

	var documentID string
	err = tx.QueryRow(ctx, `
		INSERT INTO documents (project_id, path, title, updated_by)
		VALUES ($1, $2, $2, NULLIF($3, '')::uuid)
		ON CONFLICT (project_id, path)
		DO UPDATE SET updated_at = NOW(), updated_by = NULLIF($3, '')::uuid
		RETURNING id::text
	`, projectID, payload.Document, nullableUUID(userID)).Scan(&documentID)
	if err != nil {
		return ImportJob{}, err
	}

	var versionNo int
	err = tx.QueryRow(ctx, `
		SELECT COALESCE(MAX(version_no), 0) + 1
		FROM document_versions
		WHERE document_id = $1::uuid
	`, documentID).Scan(&versionNo)
	if err != nil {
		return ImportJob{}, err
	}

	_, err = tx.Exec(ctx, `
		INSERT INTO document_versions (document_id, version_no, source_snapshot_hash, created_by)
		VALUES ($1::uuid, $2, '', NULLIF($3, '')::uuid)
	`, documentID, versionNo, nullableUUID(userID))
	if err != nil {
		return ImportJob{}, err
	}

	_, err = tx.Exec(ctx, `
		UPDATE documents
		SET current_version_no = $2
		WHERE id = $1::uuid
	`, documentID, versionNo)
	if err != nil {
		return ImportJob{}, err
	}

	var targetLanguage string
	if err := tx.QueryRow(ctx, `SELECT target_language FROM projects WHERE id = $1`, projectID).Scan(&targetLanguage); err != nil {
		return ImportJob{}, err
	}

	for _, entry := range payload.Entries {
		targetText := entry.Target[targetLanguage]
		status := entry.Meta.Status
		if status == "" {
			if targetText == "" {
				status = "untranslated"
			} else {
				status = "translated"
			}
		}

		var unitID string
		err := tx.QueryRow(ctx, `
			INSERT INTO translation_units (
				project_id,
				document_id,
				key,
				target_language,
				target_text,
				status,
				comment,
				updated_by
			)
			VALUES ($1, $2::uuid, $3, $4, $5, $6, $7, NULLIF($8, '')::uuid)
			ON CONFLICT (document_id, key)
			DO UPDATE SET target_text = EXCLUDED.target_text,
				status = EXCLUDED.status,
				comment = EXCLUDED.comment,
				version_no = translation_units.version_no + 1,
				updated_by = EXCLUDED.updated_by,
				updated_at = NOW()
			RETURNING id::text
		`, projectID, documentID, entry.Key, targetLanguage, targetText, status, entry.Meta.Comment, nullableUUID(userID)).Scan(&unitID)
		if err != nil {
			return ImportJob{}, err
		}

		for languageCode, text := range entry.Source {
			_, err := tx.Exec(ctx, `
				INSERT INTO translation_unit_sources (translation_unit_id, language_code, text)
				VALUES ($1::uuid, $2, $3)
				ON CONFLICT (translation_unit_id, language_code)
				DO UPDATE SET text = EXCLUDED.text
			`, unitID, languageCode, text)
			if err != nil {
				return ImportJob{}, err
			}
		}
	}

	var job ImportJob
	var uploadedByID *string
	var uploadedByName *string
	err = tx.QueryRow(ctx, `
		INSERT INTO import_jobs (project_id, document_path, status, uploaded_by, started_at, finished_at)
		VALUES ($1, $2, 'finished', NULLIF($3, '')::uuid, NOW(), NOW())
		RETURNING id::text, document_path, status, error_message, created_at, started_at, finished_at, NULLIF($3, '')::text, (SELECT display_name FROM users WHERE id = NULLIF($3, '')::uuid)
	`, projectID, payload.Document, nullableUUID(userID)).Scan(
		&job.ID,
		&job.DocumentPath,
		&job.Status,
		&job.ErrorMessage,
		&job.CreatedAt,
		&job.StartedAt,
		&job.FinishedAt,
		&uploadedByID,
		&uploadedByName,
	)
	if err != nil {
		return ImportJob{}, err
	}
	job.UploadedBy = userSummaryFromPointers(uploadedByID, uploadedByName)

	if err := tx.Commit(ctx); err != nil {
		return ImportJob{}, err
	}

	return job, nil
}

func (s *Store) ExportProjectDocuments(ctx context.Context, projectID string) ([]ExportDocumentPayload, error) {
	docRows, err := s.pool.Query(ctx, `
		SELECT id::text, path
		FROM documents
		WHERE project_id = $1
		ORDER BY path ASC
	`, projectID)
	if err != nil {
		return nil, err
	}
	defer docRows.Close()

	var docs []ExportDocumentPayload
	for docRows.Next() {
		var documentID string
		var payload ExportDocumentPayload
		if err := docRows.Scan(&documentID, &payload.Document); err != nil {
			return nil, err
		}

		unitRows, err := s.pool.Query(ctx, `
			SELECT
				tu.key,
				tu.target_language,
				tu.target_text,
				tu.status,
				tu.comment,
				COALESCE((
					SELECT jsonb_object_agg(tus.language_code, tus.text)
					FROM translation_unit_sources tus
					WHERE tus.translation_unit_id = tu.id
				), '{}'::jsonb) AS sources
			FROM translation_units tu
			WHERE tu.document_id = $1::uuid
			ORDER BY tu.key ASC
		`, documentID)
		if err != nil {
			return nil, err
		}

		for unitRows.Next() {
			var entry ImportEntryPayload
			var targetLanguage string
			var targetText string
			var sourcesJSON []byte
			if err := unitRows.Scan(
				&entry.Key,
				&targetLanguage,
				&targetText,
				&entry.Meta.Status,
				&entry.Meta.Comment,
				&sourcesJSON,
			); err != nil {
				unitRows.Close()
				return nil, err
			}
			if err := json.Unmarshal(sourcesJSON, &entry.Source); err != nil {
				unitRows.Close()
				return nil, err
			}
			entry.Target = map[string]string{targetLanguage: targetText}
			payload.Entries = append(payload.Entries, entry)
		}
		if err := unitRows.Err(); err != nil {
			unitRows.Close()
			return nil, err
		}
		unitRows.Close()

		docs = append(docs, payload)
	}
	return docs, docRows.Err()
}

func (s *Store) UpdateExportJobFile(ctx context.Context, projectID, exportJobID, filePath string, fileSize int64) error {
	commandTag, err := s.pool.Exec(ctx, `
		UPDATE export_jobs
		SET file_path = $3,
			file_size = $4,
			status = 'finished',
			finished_at = NOW()
		WHERE project_id = $1 AND id = $2
	`, projectID, exportJobID, filePath, fileSize)
	if err != nil {
		return err
	}
	if commandTag.RowsAffected() == 0 {
		return ErrNotFound
	}
	return nil
}

func (s *Store) ListExportJobs(ctx context.Context, projectID string) ([]ExportJob, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT
			ej.id::text,
			ej.status,
			ej.file_path,
			ej.file_size,
			ej.expires_at,
			ej.created_at,
			ej.started_at,
			ej.finished_at,
			u.id::text,
			u.display_name
		FROM export_jobs ej
		LEFT JOIN users u ON u.id = ej.requested_by
		WHERE ej.project_id = $1
		ORDER BY ej.created_at DESC
	`, projectID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var items []ExportJob
	for rows.Next() {
		var item ExportJob
		var userID *string
		var userName *string
		if err := rows.Scan(
			&item.ID,
			&item.Status,
			&item.FilePath,
			&item.FileSize,
			&item.ExpiresAt,
			&item.CreatedAt,
			&item.StartedAt,
			&item.FinishedAt,
			&userID,
			&userName,
		); err != nil {
			return nil, err
		}
		item.FileName = fileNameFromPath(item.FilePath)
		item.RequestedBy = userSummaryFromPointers(userID, userName)
		items = append(items, item)
	}
	return items, rows.Err()
}

func (s *Store) CreateExportJob(ctx context.Context, projectID, userID string) (ExportJob, error) {
	filePath := fmt.Sprintf("/exports/%s-%d.zip", projectID, time.Now().Unix())
	var item ExportJob
	var requestedByID *string
	var requestedByName *string
	err := s.pool.QueryRow(ctx, `
		INSERT INTO export_jobs (project_id, status, file_path, file_size, expires_at, requested_by, started_at, finished_at)
		VALUES ($1, 'finished', $2, 0, NOW() + INTERVAL '24 hours', NULLIF($3, '')::uuid, NOW(), NOW())
		RETURNING id::text, status, file_path, file_size, expires_at, created_at, started_at, finished_at, NULLIF($3, '')::text, (SELECT display_name FROM users WHERE id = NULLIF($3, '')::uuid)
	`, projectID, filePath, nullableUUID(userID)).Scan(
		&item.ID,
		&item.Status,
		&item.FilePath,
		&item.FileSize,
		&item.ExpiresAt,
		&item.CreatedAt,
		&item.StartedAt,
		&item.FinishedAt,
		&requestedByID,
		&requestedByName,
	)
	if err != nil {
		return ExportJob{}, err
	}
	item.FileName = fileNameFromPath(item.FilePath)
	item.RequestedBy = userSummaryFromPointers(requestedByID, requestedByName)
	return item, nil
}

func (s *Store) GetExportJob(ctx context.Context, projectID, exportJobID string) (ExportJob, error) {
	var item ExportJob
	var requestedByID *string
	var requestedByName *string
	err := s.pool.QueryRow(ctx, `
		SELECT
			ej.id::text,
			ej.status,
			ej.file_path,
			ej.file_size,
			ej.expires_at,
			ej.created_at,
			ej.started_at,
			ej.finished_at,
			u.id::text,
			u.display_name
		FROM export_jobs ej
		LEFT JOIN users u ON u.id = ej.requested_by
		WHERE ej.project_id = $1 AND ej.id = $2
	`, projectID, exportJobID).Scan(
		&item.ID,
		&item.Status,
		&item.FilePath,
		&item.FileSize,
		&item.ExpiresAt,
		&item.CreatedAt,
		&item.StartedAt,
		&item.FinishedAt,
		&requestedByID,
		&requestedByName,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return ExportJob{}, ErrNotFound
	}
	if err != nil {
		return ExportJob{}, err
	}
	item.FileName = fileNameFromPath(item.FilePath)
	item.RequestedBy = userSummaryFromPointers(requestedByID, requestedByName)
	return item, nil
}

func (s *Store) ListAuditLogs(ctx context.Context, projectID string) ([]AuditLog, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT
			al.id::text,
			al.action,
			al.resource_type,
			al.resource_id,
			al.detail_json,
			al.created_at,
			u.id::text,
			u.display_name
		FROM audit_logs al
		LEFT JOIN users u ON u.id = al.user_id
		WHERE al.project_id = $1
		ORDER BY al.created_at DESC
	`, projectID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var items []AuditLog
	for rows.Next() {
		var item AuditLog
		var userID *string
		var userName *string
		if err := rows.Scan(
			&item.ID,
			&item.Action,
			&item.ResourceType,
			&item.ResourceID,
			&item.DetailJSON,
			&item.CreatedAt,
			&userID,
			&userName,
		); err != nil {
			return nil, err
		}
		item.User = userSummaryFromPointers(userID, userName)
		items = append(items, item)
	}
	return items, rows.Err()
}

func (s *Store) resolveUnitPermissions(ctx context.Context, tx pgx.Tx, projectID string, actorID string, unit TranslationUnit) (UnitPermissions, error) {
	permissions := UnitPermissions{}

	if actorID == "" {
		return permissions, nil
	}

	rolePermissions := map[string]bool{}
	rows, err := tx.Query(ctx, `
		SELECT pn.code
		FROM project_members pm
		JOIN project_roles pr ON pr.project_id = pm.project_id AND pr.code = pm.role_code
		JOIN project_role_permissions prp ON prp.project_role_id = pr.id
		JOIN permission_nodes pn ON pn.id = prp.permission_node_id
		WHERE pm.project_id = $1 AND pm.user_id = $2::uuid
	`, projectID, actorID)
	if err != nil {
		return permissions, err
	}
	for rows.Next() {
		var code string
		if err := rows.Scan(&code); err != nil {
			rows.Close()
			return permissions, err
		}
		rolePermissions[code] = true
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return permissions, err
	}

	overrideRows, err := tx.Query(ctx, `
		SELECT pn.code, po.effect
		FROM project_member_permission_overrides po
		JOIN permission_nodes pn ON pn.id = po.permission_node_id
		WHERE po.project_id = $1 AND po.user_id = $2::uuid
	`, projectID, actorID)
	if err != nil {
		return permissions, err
	}
	for overrideRows.Next() {
		var code, effect string
		if err := overrideRows.Scan(&code, &effect); err != nil {
			overrideRows.Close()
			return permissions, err
		}
		rolePermissions[code] = effect == "allow"
	}
	overrideRows.Close()
	if err := overrideRows.Err(); err != nil {
		return permissions, err
	}

	allowWrite := true
	denyWrite := false

	ruleRows, err := tx.Query(ctx, `
		SELECT permission_scope, match_type, match_value, effect
		FROM project_member_document_rules
		WHERE project_id = $1 AND user_id = $2::uuid
	`, projectID, actorID)
	if err != nil {
		return permissions, err
	}
	for ruleRows.Next() {
		var scope, matchType, matchValue, effect string
		if err := ruleRows.Scan(&scope, &matchType, &matchValue, &effect); err != nil {
			ruleRows.Close()
			return permissions, err
		}
		if scope != "translation.edit" && scope != "translation.review" && scope != "translation.approve" {
			continue
		}
		if !documentRuleMatches(unit.Document, matchType, matchValue) {
			continue
		}
		if effect == "deny" {
			denyWrite = true
		}
		if effect == "allow" {
			allowWrite = true
		}
	}
	ruleRows.Close()
	if err := ruleRows.Err(); err != nil {
		return permissions, err
	}

	permissions.CanView = rolePermissions["translation.view"] || rolePermissions["project.view"] || rolePermissions["document.view"]
	permissions.CanEdit = rolePermissions["translation.edit"] && allowWrite && !denyWrite
	permissions.CanReview = rolePermissions["translation.review"] && allowWrite && !denyWrite
	permissions.CanApprove = rolePermissions["translation.approve"] && allowWrite && !denyWrite

	return permissions, nil
}

func (s *Store) UpdateTranslationUnit(ctx context.Context, projectID, unitID string, input UpdateTranslationUnitInput) (TranslationUnit, error) {
	tx, err := s.pool.BeginTx(ctx, pgx.TxOptions{})
	if err != nil {
		return TranslationUnit{}, err
	}
	defer tx.Rollback(ctx)

	var unit struct {
		ID             string
		DocumentID     string
		TargetText     string
		Status         string
		VersionNo      int
		CurrentMaxRev  int
	}

	err = tx.QueryRow(ctx, `
		SELECT
			tu.id::text,
			tu.document_id::text,
			tu.target_text,
			tu.status,
			tu.version_no,
			COALESCE((SELECT MAX(tr.revision_no) FROM translation_revisions tr WHERE tr.translation_unit_id = tu.id), 0)
		FROM translation_units tu
		WHERE tu.project_id = $1 AND tu.id = $2
		FOR UPDATE
	`, projectID, unitID).Scan(
		&unit.ID,
		&unit.DocumentID,
		&unit.TargetText,
		&unit.Status,
		&unit.VersionNo,
		&unit.CurrentMaxRev,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return TranslationUnit{}, ErrNotFound
	}
	if err != nil {
		return TranslationUnit{}, err
	}

	newVersion := unit.VersionNo + 1
	_, err = tx.Exec(ctx, `
		UPDATE translation_units
		SET target_text = $3,
			status = $4,
			comment = $5,
			version_no = $6,
			updated_by = NULLIF($7, '')::uuid,
			updated_at = NOW()
		WHERE project_id = $1 AND id = $2
	`, projectID, unitID, input.TargetText, input.Status, input.Comment, newVersion, nullableUUID(input.ActorID))
	if err != nil {
		return TranslationUnit{}, err
	}

	_, err = tx.Exec(ctx, `
		INSERT INTO translation_revisions (
			translation_unit_id,
			revision_no,
			before_target_text,
			after_target_text,
			before_status,
			after_status,
			change_note,
			changed_by
		) VALUES ($1, $2, $3, $4, $5, $6, $7, NULLIF($8, '')::uuid)
	`, unitID, unit.CurrentMaxRev+1, unit.TargetText, input.TargetText, unit.Status, input.Status, input.ChangeNote, nullableUUID(input.ActorID))
	if err != nil {
		return TranslationUnit{}, err
	}

	_, err = tx.Exec(ctx, `
		UPDATE documents
		SET updated_at = NOW(),
			updated_by = NULLIF($2, '')::uuid
		WHERE id = $1
	`, unit.DocumentID, nullableUUID(input.ActorID))
	if err != nil {
		return TranslationUnit{}, err
	}

	if err := tx.Commit(ctx); err != nil {
		return TranslationUnit{}, err
	}

	return s.GetTranslationUnit(ctx, projectID, unitID, input.ActorID)
}

func (s *Store) CreateTag(ctx context.Context, projectID string, input CreateTagInput) (DocumentTag, error) {
	var item DocumentTag
	err := s.pool.QueryRow(ctx, `
		INSERT INTO document_tags (project_id, code, name, color, is_visible)
		VALUES ($1, $2, $3, $4, $5)
		RETURNING id::text, code, name, color, is_visible
	`, projectID, input.Code, input.Name, input.Color, input.IsVisible).Scan(
		&item.ID,
		&item.Code,
		&item.Name,
		&item.Color,
		&item.IsVisible,
	)
	return item, err
}

func (s *Store) UpdateTag(ctx context.Context, projectID, tagID string, input UpdateTagInput) (DocumentTag, error) {
	var item DocumentTag
	err := s.pool.QueryRow(ctx, `
		UPDATE document_tags
		SET name = $3,
			color = $4,
			is_visible = $5
		WHERE project_id = $1 AND id = $2
		RETURNING id::text, code, name, color, is_visible
	`, projectID, tagID, input.Name, input.Color, input.IsVisible).Scan(
		&item.ID,
		&item.Code,
		&item.Name,
		&item.Color,
		&item.IsVisible,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return DocumentTag{}, ErrNotFound
	}
	return item, err
}

func (s *Store) BindTagToDocument(ctx context.Context, projectID, documentID, tagID string) error {
	commandTag, err := s.pool.Exec(ctx, `
		INSERT INTO document_tag_bindings (document_id, tag_id)
		SELECT $2::uuid, $3::uuid
		WHERE EXISTS (
			SELECT 1
			FROM documents d
			JOIN document_tags t ON t.project_id = d.project_id
			WHERE d.project_id = $1
			  AND d.id = $2::uuid
			  AND t.id = $3::uuid
		)
		ON CONFLICT (document_id, tag_id) DO NOTHING
	`, projectID, documentID, tagID)
	if err != nil {
		return err
	}
	if commandTag.RowsAffected() == 0 {
		return ErrNotFound
	}
	return nil
}

func (s *Store) UnbindTagFromDocument(ctx context.Context, projectID, documentID, tagID string) error {
	commandTag, err := s.pool.Exec(ctx, `
		DELETE FROM document_tag_bindings dtb
		USING documents d, document_tags t
		WHERE dtb.document_id = d.id
		  AND dtb.tag_id = t.id
		  AND d.project_id = $1
		  AND d.id = $2::uuid
		  AND t.id = $3::uuid
	`, projectID, documentID, tagID)
	if err != nil {
		return err
	}
	if commandTag.RowsAffected() == 0 {
		return ErrNotFound
	}
	return nil
}

type scanner interface {
	Scan(dest ...any) error
}

func scanTranslationUnit(row scanner) (TranslationUnit, error) {
	var item TranslationUnit
	var documentID string
	var documentPath string
	var tagsJSON []byte
	var sourcesJSON []byte
	var userID *string
	var userName *string

	err := row.Scan(
		&item.ID,
		&item.Key,
		&documentID,
		&documentPath,
		&tagsJSON,
		&sourcesJSON,
		&item.Target.Language,
		&item.Target.Text,
		&item.Status,
		&item.Comment,
		&item.Version,
		&item.UpdatedAt,
		&userID,
		&userName,
	)
	if err != nil {
		return TranslationUnit{}, err
	}

	if err := json.Unmarshal(tagsJSON, &item.Document.Tags); err != nil {
		return TranslationUnit{}, err
	}
	if err := json.Unmarshal(sourcesJSON, &item.Sources); err != nil {
		return TranslationUnit{}, err
	}

	item.Document.ID = documentID
	item.Document.Path = documentPath
	item.UpdatedBy = userSummaryFromPointers(userID, userName)
	item.Permissions = UnitPermissions{}

	return item, nil
}

func userSummaryFromPointers(id *string, name *string) *UserSummary {
	if id == nil || name == nil {
		return nil
	}
	return &UserSummary{
		ID:   *id,
		Name: *name,
	}
}

func nullableUUID(value string) string {
	return value
}

func uniqueNonEmptyStrings(values []string) []string {
	seen := map[string]struct{}{}
	result := make([]string, 0, len(values))
	for _, value := range values {
		if value == "" {
			continue
		}
		if _, ok := seen[value]; ok {
			continue
		}
		seen[value] = struct{}{}
		result = append(result, value)
	}
	return result
}

func documentRuleMatches(document DocumentRef, matchType string, matchValue string) bool {
	switch matchType {
	case "path_prefix":
		return strings.HasPrefix(document.Path, matchValue)
	case "document_id":
		return document.ID == matchValue
	case "tag":
		for _, tag := range document.Tags {
			if tag.Code == matchValue || tag.Name == matchValue {
				return true
			}
		}
		return false
	default:
		return false
	}
}


func randomToken() (string, error) {
	buffer := make([]byte, 32)
	if _, err := rand.Read(buffer); err != nil {
		return "", err
	}
	return hex.EncodeToString(buffer), nil
}

func fileNameFromPath(path string) string {
	if path == "" {
		return ""
	}
	parts := strings.Split(path, "/")
	return parts[len(parts)-1]
}

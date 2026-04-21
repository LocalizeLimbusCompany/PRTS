package httpserver

import (
	"net/http"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/go-chi/chi/v5/middleware"

	"prts-translation-system/internal/config"
	"prts-translation-system/internal/handlers"
	custommiddleware "prts-translation-system/internal/middleware"
	"prts-translation-system/internal/runtime"
)

type Server struct {
	router chi.Router
}

func New(cfg config.Config, runtime *runtime.Runtime) *Server {
	r := chi.NewRouter()

	r.Use(middleware.RequestID)
	r.Use(middleware.RealIP)
	r.Use(middleware.Recoverer)
	r.Use(middleware.Timeout(30 * time.Second))
	r.Use(custommiddleware.JSONContentType)
	r.Use(custommiddleware.OptionalAuth(runtime.Store))

	r.Get("/healthz", handlers.Health)
	r.Get("/readyz", handlers.Ready)
	r.Handle("/uploads/*", http.StripPrefix("/uploads/", http.FileServer(http.Dir(cfg.Upload.Dir))))

	r.Route("/api/v1", func(api chi.Router) {
		api.Get("/", handlers.APIIndex(cfg))
		api.Get("/meta", handlers.Meta(cfg))
		api.Post("/auth/login", handlers.Login(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Post("/auth/logout", handlers.Logout(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Get("/me", handlers.Me())
		api.With(custommiddleware.RequireAuth(runtime.Store)).Patch("/me", handlers.UpdateMyProfile(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Patch("/me/preferences", handlers.UpdateMyPreferences(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Post("/me/avatar", handlers.UploadMyAvatar(runtime))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Get("/admin/overview", handlers.GetPlatformOverview(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Get("/admin/settings", handlers.GetPlatformSettings(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Patch("/admin/settings", handlers.UpdatePlatformSettings(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Get("/admin/users", handlers.ListPlatformUsers(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Patch("/admin/users/{userId}", handlers.UpdatePlatformUser(runtime.Store))
		api.Get("/organizations", handlers.ListOrganizations(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Post("/organizations", handlers.CreateOrganization(runtime.Store))
		api.Get("/organizations/{organizationId}", handlers.GetOrganization(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Patch("/organizations/{organizationId}", handlers.UpdateOrganization(runtime.Store))
		api.Get("/organizations/{organizationId}/projects", handlers.ListProjectsByOrganization(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Post("/organizations/{organizationId}/projects", handlers.CreateProject(runtime.Store))
		api.Get("/projects/{projectId}", handlers.GetProject(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Patch("/projects/{projectId}", handlers.UpdateProject(runtime.Store))
		api.Get("/projects/{projectId}/members", handlers.ListProjectMembers(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Post("/projects/{projectId}/members", handlers.UpsertProjectMember(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Delete("/projects/{projectId}/members/{userId}", handlers.DeleteProjectMember(runtime.Store))
		api.Get("/projects/{projectId}/roles", handlers.ListProjectRoles(runtime.Store))
		api.Get("/projects/{projectId}/permissions", handlers.ListProjectPermissions(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Patch("/projects/{projectId}/members/{userId}/permissions", handlers.SetProjectMemberPermissions(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Delete("/projects/{projectId}/members/{userId}/permissions", handlers.DeleteProjectMemberPermission(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Patch("/projects/{projectId}/members/{userId}/document-rules", handlers.SetProjectMemberDocumentRules(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Delete("/projects/{projectId}/members/{userId}/document-rules", handlers.DeleteProjectMemberDocumentRule(runtime.Store))
		api.Get("/projects/{projectId}/tags", handlers.ListTags(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Post("/projects/{projectId}/tags", handlers.CreateTag(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Patch("/projects/{projectId}/tags/{tagId}", handlers.UpdateTag(runtime.Store))
		api.Get("/projects/{projectId}/documents", handlers.ListDocuments(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Post("/projects/{projectId}/documents", handlers.CreateDocument(runtime.Store))
		api.Get("/projects/{projectId}/documents/{documentId}", handlers.GetDocument(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Patch("/projects/{projectId}/documents/{documentId}", handlers.UpdateDocument(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Post("/projects/{projectId}/documents/{documentId}/tags", handlers.BindTagToDocument(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Delete("/projects/{projectId}/documents/{documentId}/tags/{tagId}", handlers.UnbindTagFromDocument(runtime.Store))
		api.Get("/projects/{projectId}/documents/{documentId}/versions", handlers.ListDocumentVersions(runtime.Store))
		api.Get("/projects/{projectId}/history", handlers.ListProjectHistory(runtime.Store))
		api.Get("/projects/{projectId}/units", handlers.ListTranslationUnits(runtime.Store))
		api.Get("/projects/{projectId}/units/{unitId}", handlers.GetTranslationUnit(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Patch("/projects/{projectId}/units/{unitId}", handlers.UpdateTranslationUnit(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Post("/projects/{projectId}/units/{unitId}/review", handlers.ReviewTranslationUnit(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Post("/projects/{projectId}/units/{unitId}/approve", handlers.ApproveTranslationUnit(runtime.Store))
		api.Get("/projects/{projectId}/units/{unitId}/history", handlers.ListTranslationUnitHistory(runtime.Store))
		api.Get("/projects/{projectId}/glossary", handlers.ListGlossaryTerms(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Post("/projects/{projectId}/glossary", handlers.CreateGlossaryTerm(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Patch("/projects/{projectId}/glossary/{termId}", handlers.UpdateGlossaryTerm(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Delete("/projects/{projectId}/glossary/{termId}", handlers.DeleteGlossaryTerm(runtime.Store))
		api.Get("/projects/{projectId}/tm", handlers.ListTMEntries(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Post("/projects/{projectId}/tm", handlers.CreateTMEntry(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Patch("/projects/{projectId}/tm/{entryId}", handlers.UpdateTMEntry(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Delete("/projects/{projectId}/tm/{entryId}", handlers.DeleteTMEntry(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Post("/projects/{projectId}/imports", handlers.CreateImportJob(runtime.Store))
		api.Get("/projects/{projectId}/imports", handlers.ListImportJobs(runtime.Store))
		api.With(custommiddleware.RequireAuth(runtime.Store)).Post("/projects/{projectId}/exports", handlers.CreateExportJob(runtime))
		api.Get("/projects/{projectId}/exports", handlers.ListExportJobs(runtime))
		api.Get("/projects/{projectId}/exports/{exportJobId}/download", handlers.DownloadExportJob(runtime))
		api.Get("/projects/{projectId}/audit-logs", handlers.ListAuditLogs(runtime.Store))
	})

	return &Server{router: r}
}

func (s *Server) Handler() http.Handler {
	return s.router
}

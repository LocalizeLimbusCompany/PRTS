param(
    [switch]$IncludeManualScale,
    [switch]$IncludeDatabaseChecks
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$RepositoryRoot = (git rev-parse --show-toplevel).Trim()
if (-not $RepositoryRoot) { throw 'Not inside a Git worktree.' }
Set-Location -LiteralPath $RepositoryRoot

$Failures = [System.Collections.Generic.List[string]]::new()
$PassCount = 0

function Add-Failure([string]$Message) {
    $script:Failures.Add($Message)
    Write-Host "[FAIL] $Message" -ForegroundColor Red
}

function Add-Pass([string]$Message) {
    $script:PassCount++
    Write-Host "[PASS] $Message" -ForegroundColor Green
}

function Assert-True([bool]$Condition, [string]$Message) {
    if ($Condition) { Add-Pass $Message } else { Add-Failure $Message }
}

function Assert-Contains([string]$Path, [string]$Pattern, [string]$Message) {
    $content = Get-Content -LiteralPath $Path -Raw
    Assert-True ([regex]::IsMatch($content, $Pattern, [System.Text.RegularExpressions.RegexOptions]::Multiline)) $Message
}

function Assert-NotContains([string]$Path, [string]$Pattern, [string]$Message) {
    $content = Get-Content -LiteralPath $Path -Raw
    Assert-True (-not [regex]::IsMatch($content, $Pattern, [System.Text.RegularExpressions.RegexOptions]::Multiline)) $Message
}

function Test-MarkdownLinks {
    $linkPattern = [regex]'!?(?:\[[^\]]*\])\((?<target>[^)]+)\)'
    $markdownFiles = Get-ChildItem -LiteralPath . -Recurse -File -Include '*.md' |
        Where-Object { $_.FullName -notmatch '[\\/](target|node_modules|dist)[\\/]' }
    foreach ($file in $markdownFiles) {
        $content = Get-Content -LiteralPath $file.FullName -Raw
        foreach ($match in $linkPattern.Matches($content)) {
            $target = $match.Groups['target'].Value.Trim().Trim('<', '>')
            if ($target -match '^(?:https?|mailto):' -or $target.StartsWith('#')) { continue }
            $target = ($target -split '#', 2)[0]
            if ([string]::IsNullOrWhiteSpace($target)) { continue }
            $decoded = [System.Uri]::UnescapeDataString($target)
            $resolved = Join-Path -Path $file.DirectoryName -ChildPath $decoded
            if (-not (Test-Path -LiteralPath $resolved)) {
                Add-Failure "Broken Markdown relative link: $($file.FullName.Substring($RepositoryRoot.Length + 1)) -> $target"
            }
        }
    }
    if (-not ($Failures | Where-Object { $_ -like 'Broken Markdown relative link:*' })) {
        Add-Pass 'Markdown relative links resolve to local targets'
    }
}

function Test-PlanPaths {
    $planPath = 'docs/superpowers/plans/2026-07-10-project-workspace-overhaul.md'
    $latestActions = @{}
    foreach ($line in Get-Content -LiteralPath $planPath) {
        if ($line -match '^- (Create|Modify|Test|Delete): `([^`]+)`') {
            $latestActions[$Matches[2]] = $Matches[1]
        }
    }
    foreach ($entry in $latestActions.GetEnumerator()) {
        $exists = Test-Path -LiteralPath $entry.Key
        if ($entry.Value -eq 'Delete') {
            if ($exists) { Add-Failure "Plan Delete path still exists: $($entry.Key)" }
        } elseif (-not $exists) {
            Add-Failure "Plan $($entry.Value) path is missing: $($entry.Key)"
        }
    }
    if (-not ($Failures | Where-Object { $_ -like 'Plan *' })) {
        Add-Pass 'Plan Create/Modify/Test/Delete paths match the final worktree'
    }
}

function Test-ConflictKeywords {
    $patterns = @(
        'scope: all',
        '永久保存行为',
        'entry history 本来不保存 context',
        '断流.*续传',
        'source_langs\[1\].*运行时',
        'audit.*旁路',
        '先删媒体.*项目',
        'NUMERIC\(20,1\)',
        'BigDecimal',
        'rust_decimal',
        'file_id:\s*Uu(?:id|ID)',
        'task_id:\s*Uu(?:id|ID)'
    )
    $targets = @('docs', 'plan', 'README.md', 'README.en.md')
    $excludedPlan = 'docs/superpowers/plans/2026-07-10-project-workspace-overhaul.md'
    foreach ($target in $targets) {
        $files = if (Test-Path -LiteralPath $target -PathType Container) {
            Get-ChildItem -LiteralPath $target -Recurse -File
        } else {
            Get-Item -LiteralPath $target
        }
        foreach ($file in $files) {
            $relative = $file.FullName.Substring($RepositoryRoot.Length + 1).Replace('\', '/')
            if ($relative -eq $excludedPlan) { continue }
            $content = Get-Content -LiteralPath $file.FullName -Raw
            foreach ($pattern in $patterns) {
                foreach ($match in [regex]::Matches($content, $pattern, [System.Text.RegularExpressions.RegexOptions]::IgnoreCase)) {
                    $lineStart = $content.LastIndexOf("`n", [Math]::Max(0, $match.Index - 1)) + 1
                    $lineEnd = $content.IndexOf("`n", $match.Index)
                    if ($lineEnd -lt 0) { $lineEnd = $content.Length }
                    $line = $content.Substring($lineStart, $lineEnd - $lineStart)
                    $historicalOverride = $line -match '历史实现基线' -and $line -match '已由.+取代'
                    if (-not $historicalOverride) {
                        Add-Failure "Conflicting current-rule keyword '$pattern' in $relative"
                    }
                }
            }
        }
    }
    if (-not ($Failures | Where-Object { $_ -like 'Conflicting current-rule keyword*' })) {
        Add-Pass 'Documentation conflict keyword scan is clean'
    }
}

function Test-Contracts {
    Assert-Contains 'backend/crates/prts-core/src/search_query.rs' '#\[serde\(tag = "type", rename_all = "snake_case", deny_unknown_fields\)\]' 'Runtime SearchScope is a strict type-tagged union'
    Assert-Contains 'backend/crates/prts-api/src/routes/search.rs' 'File \{ file_id: i64 \}' 'Search file scope uses i64'
    Assert-Contains 'backend/crates/prts-api/src/routes/search.rs' 'CurrentTask \{ task_id: i64 \}' 'Search task scope uses i64'
    Assert-Contains 'backend/crates/prts-api/src/routes/entries.rs' '#\[deprecated\(note = "use the streaming upload-batches API"\)\]' 'Legacy upload is deprecated in OpenAPI generation'
    Assert-Contains 'backend/crates/prts-api/src/routes/search.rs' '#\[deprecated\(note = "use POST /projects/\{id\}/search"\)\]' 'Legacy GET search is deprecated in OpenAPI generation'
    Assert-Contains 'backend/crates/prts-api/src/routes/search.rs' 'Discriminator::new\("type"\)' 'OpenAPI SearchScope declares the type discriminator'
    Assert-Contains 'backend/crates/prts-api/src/routes/search.rs' 'AdditionalProperties::FreeForm\(false\)' 'OpenAPI SearchScope variants reject unknown properties'
    Assert-Contains 'frontend/src/api/index.ts' '\.post<StructuredSearchResponse>\(`\/projects\/\$\{id\}\/search`, body\)' 'Frontend structured search uses POST'
    Assert-NotContains 'frontend/src/api/index.ts' '\.get<StructuredSearchResponse>\(`\/projects\/\$\{id\}\/search`' 'Frontend does not expose legacy GET search'
    Assert-Contains 'frontend/src/views/project/ProjectFilesView.vue' 'UploadBatchDialog' 'Workspace file UI uses upload batches'
    Assert-NotContains 'frontend/src/views/project/ProjectFilesView.vue' 'entriesApi\.upload' 'Workspace file UI does not call legacy upload'
    Assert-Contains 'backend/crates/prts-api/src/routes/entries.rs' 'canonicalize_language_tag' 'Legacy upload BCP-47 ingress uses the shared canonicalizer'
    Assert-Contains 'backend/crates/prts-api/src/jobs/process_upload.rs' 'canonicalize_language_tag' 'Streaming upload BCP-47 ingress uses the shared canonicalizer'
    Assert-Contains 'backend/crates/prts-core/src/search_query.rs' 'canonicalize_language_tag' 'Search source selector uses the shared canonicalizer'
    Assert-Contains 'backend/crates/prts-core/src/terms.rs' 'canonicalize_language_tag' 'Term ingress uses the shared canonicalizer'
    Assert-Contains 'backend/crates/prts-api/src/routes/users.rs' 'canonicalize_language_tags' 'User language preferences use the shared canonicalizer'
    Assert-NotContains 'backend/crates/prts-api/src/dto.rs' 'pub\s+context\s*:' 'Entry API DTO does not expose context'
    Assert-NotContains 'frontend/src/api/types.ts' '^\s*context\s*:' 'Frontend entry schema does not expose context'
    Assert-Contains 'backend/migrations/0013_editor_search.sql' "before_value - 'context'" 'Migration scrubs context from history before drop'
    Assert-Contains 'backend/migrations/0013_editor_search.sql' 'DROP COLUMN context' 'Migration removes entries.context'
    Assert-True ((Get-ChildItem -LiteralPath 'backend/migrations' -File | Sort-Object Name | Select-Object -Last 1).Name -eq '0014_admin_delete_cp.sql') '0014 remains the newest migration'
}

function Test-ScaleRecoverySecurityContracts {
    Assert-Contains 'backend/crates/prts-api/tests/search_perf.rs' 'search_perf_five_scopes_and_materialized_reads' 'Manual search verify covers all five scopes and materialized reads'
    Assert-Contains 'backend/crates/prts-api/tests/search_perf.rs' 'PRTS_PERF_N' 'Manual search scale is explicitly configurable to 200k entries'
    Assert-Contains 'backend/crates/prts-api/tests/search_perf.rs' 'PRTS_SEARCH_BUDGET_MS' 'Manual search verify enforces an explicit latency budget'
    Assert-Contains 'backend/crates/prts-api/tests/search_perf.rs' 'stats_and_task_progress_hot_reads_only_use_materialized_tables' 'CI checks stats and task progress hot-read SQL contracts'
    Assert-Contains 'backend/crates/prts-api/tests/search_perf.rs' 'lexical_reindex_persists_a_keyset_checkpoint_every_bounded_batch' 'CI checks lexical keyset checkpoint recovery'
    Assert-Contains 'backend/crates/prts-api/tests/upload_perf.rs' 'max_files_per_batch: 500' 'CI checks the 500-file upload contract'
    Assert-Contains 'backend/crates/prts-api/tests/upload_perf.rs' 'max_bytes_per_batch: 2 \* GIBIBYTE' 'CI checks the 2GiB upload contract'
    Assert-Contains 'backend/crates/prts-api/src/jobs/process_upload.rs' 'upload_perf_parser_streams_large_file_through_bounded_channel' 'Manual upload verify covers bounded 100MiB parsing'
    Assert-Contains 'backend/crates/prts-api/tests/upload_perf.rs' 'byte_zero_attempt_lifecycle' 'CI checks byte-zero retry and attempt history'
    Assert-Contains 'backend/crates/prts-api/tests/upload_perf.rs' 'cancellation_expiry_partial_success_and_cleanup' 'CI checks cancellation, expiry, partial success and cleanup'
    Assert-Contains 'backend/crates/prts-api/tests/upload_perf.rs' 'replacement_history_and_thirty_day_purge' 'CI checks replacement, rollback and restoration purge contracts'
    Assert-Contains 'backend/crates/prts-api/tests/upload_perf.rs' 'stage8_recovery_wiring_resumes_durable_stages_without_leaking_internal_errors' 'CI checks durable recovery wiring and redacted worker errors'

    $integration = 'backend/crates/prts-api/tests/db_integration.rs'
    foreach ($contract in @(
        'language_resolution_owner_selection_permissions_and_audit_rollback_are_atomic',
        'upload_replacement_duplicate_key_rolls_back_file_transaction',
        'audit_jobs_expired_lease_is_taken_over_and_active_lease_is_preserved',
        'audit_contract_transient_auth_outbox_requeues_same_intent_until_redis_returns',
        'file_history_retention_purge_uses_explicit_cleanup_order',
        'stage7_due_purge_is_db_first_retains_job_and_retries_external_cleanup_idempotently',
        'structured_search_scopes_conditions_visibility_cursor_and_get_adapter_are_stable',
        'terminology_import_preview_confirm_is_bound_one_time_and_fail_closed',
        'stage7_membership_authorization_matrix_and_owner_binding_fail_closed',
        'audit_contract_projects_files_entries_memberships_and_export_are_audited_and_redacted',
        'public_editor_is_anonymous_read_only_and_private_editor_fails_closed'
    )) {
        Assert-Contains $integration $contract "DB integration coverage exists: $contract"
    }
    Assert-Contains 'backend/crates/prts-api/src/jobs/reindex_project.rs' 'primary_source_embedding_backfill' 'Lexical and embedding recovery use separate durable stages'
    Assert-Contains 'backend/crates/prts-api/src/jobs/purge_project.rs' 'external_cleanup' 'Project purge keeps a retryable external-cleanup stage'
    Assert-Contains 'backend/crates/prts-db/src/audit.rs' 'pub enum AuditEvent' 'Audit payload construction uses a closed action-specific allowlist'
    Assert-Contains 'backend/crates/prts-api/src/auth/project.rs' 'deletion_scheduled_at' 'Pending-deletion access is enforced by the shared project guard'
}

function Test-ReleaseCompatibilityContracts {
    Assert-Contains 'backend/crates/prts-api/src/routes/entries.rs' 'compatibility_endpoint = "legacy_upload"' 'Legacy upload emits a stable compatibility usage event'
    Assert-Contains 'backend/crates/prts-api/src/routes/search.rs' 'compatibility_endpoint = "legacy_get_search"' 'Legacy GET search emits a stable compatibility usage event'
    Assert-Contains '.github/workflows/ci.yml' 'verify-project-workspace.ps1' 'CI runs the workspace contract verifier'
    Assert-Contains 'deploy/nginx/default.conf' 'client_max_body_size 100m;' 'nginx accepts the documented per-file upload ceiling'
    Assert-Contains 'deploy/nginx/default.conf' 'proxy_request_buffering off;' 'nginx streams API request bodies to the backend'
    Assert-Contains 'deploy/docker-compose.yml' 'condition: service_healthy' 'Compose gates dependent services on health checks'
    Assert-Contains 'deploy/docker-compose.yml' 'curl -fsS http://localhost:3000/health/ready' 'Compose verifies backend dependency readiness'
}

function Invoke-Checked([string]$Label, [scriptblock]$Command) {
    Write-Host "[RUN ] $Label" -ForegroundColor Cyan
    & $Command
    if ($LASTEXITCODE -ne 0) { Add-Failure "$Label exited with $LASTEXITCODE" } else { Add-Pass $Label }
}

Test-MarkdownLinks
Test-PlanPaths
Test-ConflictKeywords
Test-Contracts
Test-ScaleRecoverySecurityContracts
Test-ReleaseCompatibilityContracts

if ($IncludeDatabaseChecks) {
    Push-Location backend
    Invoke-Checked 'DB integration contract tests' { cargo test -p prts-api --features db-tests }
    Pop-Location
} else {
    Write-Host '[SKIP] DB integration tests; pass -IncludeDatabaseChecks after PostgreSQL/Redis are ready.' -ForegroundColor Yellow
}

if ($IncludeManualScale) {
    Push-Location backend
    Invoke-Checked 'Manual ignored search scale tests' { cargo test -p prts-api --features db-tests -- --ignored search_perf --nocapture }
    Invoke-Checked 'Manual ignored upload scale tests' { cargo test -p prts-api --features db-tests -- --ignored upload_perf --nocapture }
    Pop-Location
} else {
    Write-Host '[SKIP] Expensive scale tests; pass -IncludeManualScale and set PRTS_PERF_N/PRTS_UPLOAD_PERF_N for measured runs.' -ForegroundColor Yellow
}

if ($Failures.Count -gt 0) {
    Write-Host "Verification failed: $($Failures.Count) failure(s), $PassCount pass(es)." -ForegroundColor Red
    exit 1
}

Write-Host "Verification passed: $PassCount checks. Manual scale results were not claimed unless explicitly requested." -ForegroundColor Green

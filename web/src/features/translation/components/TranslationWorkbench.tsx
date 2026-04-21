import { useEffect, useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { AlertCircle, CheckCheck, EyeOff, FileText, LoaderCircle, Lock, Save, Search, ShieldCheck, ArrowLeft, CircleHelp, SlidersHorizontal, X } from 'lucide-react';

import { api } from '@/api/client';
import { cn } from '@/lib/utils';
import { useAuthStore } from '@/store/auth';
import { useTranslation } from '@/hooks/useTranslation';
import ContextSidebar from './ContextSidebar';

interface Props {
  projectId: string;
  documentId?: string;
}

interface DocumentTag {
  code: string;
  name: string;
  color: string;
}

interface DocumentItem {
  id: string;
  path: string;
  title: string;
  tags?: DocumentTag[];
}

interface UserSummary {
  id: string;
  name: string;
}

interface UnitPermissions {
  canView?: boolean;
  canEdit?: boolean;
  canReview?: boolean;
  canApprove?: boolean;
}

interface TranslationUnit {
  id: string;
  key: string;
  document: {
    id: string;
    path: string;
    tags?: DocumentTag[];
  };
  sources: Record<string, string>;
  target: {
    language: string;
    text: string;
  };
  status: string;
  isQuestioned?: boolean;
  isLocked?: boolean;
  isHidden?: boolean;
  comment: string;
  updatedAt: string;
  updatedBy?: UserSummary;
  permissions?: UnitPermissions;
}

interface UnitsResponse {
  items: TranslationUnit[];
  total: number;
}

const quickStatuses = ['untranslated', 'translated', 'reviewed', 'approved'] as const;
type AdvancedCondition = {
  field: string;
  operator: string;
  value: string;
};
type SearchMode = 'source_all' | 'target' | 'source_and_target';

export default function TranslationWorkbench({ projectId, documentId: propDocumentId }: Props) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const user = useAuthStore((s) => s.user);
  const preferredSource = normalizePreferredSource(user?.preferredSourceLanguage || 'en');
  const [activeUnitId, setActiveUnitId] = useState<string | null>(null);
  const [draftText, setDraftText] = useState('');
  const [searchText, setSearchText] = useState('');
  const [searchMode, setSearchMode] = useState<SearchMode>('source_and_target');
  const [statusFilters, setStatusFilters] = useState<string[]>([]);
  const [scope, setScope] = useState<'current_document' | 'all_documents'>('current_document');
  const [onlyQuestioned, setOnlyQuestioned] = useState(false);
  const [onlyLocked, setOnlyLocked] = useState(false);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [advancedConditions, setAdvancedConditions] = useState<AdvancedCondition[]>([{ field: 'target', operator: 'contains', value: '' }]);

  const selectedDocId = propDocumentId;

  const { data: docsData, isLoading: isLoadingDocs } = useQuery({
    queryKey: ['documents', projectId],
    queryFn: () => api.get<{ items: DocumentItem[] }>(`/projects/${projectId}/documents`),
  });

  const documents = Array.isArray(docsData?.items) ? docsData.items : [];
  const selectedDocument = documents.find((doc) => doc.id === selectedDocId) ?? null;

  const unitsQuery = useMemo(() => {
    const params = new URLSearchParams({ page: '1', pageSize: '200' });
    if (scope === 'current_document' && selectedDocId) params.set('documentId', selectedDocId);
    params.set('scope', scope);
    if (searchText.trim()) {
      params.set('q', searchText.trim());
    }
    statusFilters.forEach((item) => params.append('statuses', item));
    if (onlyQuestioned) params.set('isQuestioned', 'true');
    if (onlyLocked) params.set('isLocked', 'true');
    if (showAdvanced) {
      params.set('advanced', JSON.stringify(advancedConditions.filter((item) => item.value.trim())));
    }
    return params.toString();
  }, [scope, selectedDocId, searchText, searchMode, statusFilters, onlyQuestioned, onlyLocked, showAdvanced, advancedConditions]);

  const { data: unitsData, isLoading: isLoadingUnits } = useQuery({
    queryKey: ['workbench-units', projectId, unitsQuery],
    enabled: scope === 'all_documents' || Boolean(selectedDocId),
    queryFn: () => api.get<UnitsResponse>(`/projects/${projectId}/units?${unitsQuery}`),
  });

  const units = Array.isArray(unitsData?.items) ? unitsData.items : [];

  useEffect(() => {
    if (!units.length) {
      setActiveUnitId(null);
      return;
    }
    setActiveUnitId((current) => (current && units.some((unit) => unit.id === current) ? current : units[0].id));
  }, [units]);

  const activeUnit = useMemo(
    () => units.find((unit) => unit.id === activeUnitId) ?? null,
    [activeUnitId, units],
  );

  useEffect(() => {
    setDraftText(activeUnit?.target?.text || '');
  }, [activeUnit]);

  const refreshUnits = () => {
    void queryClient.invalidateQueries({ queryKey: ['workbench-units', projectId] });
    if (activeUnitId) {
      void queryClient.invalidateQueries({ queryKey: ['unit-history', projectId, activeUnitId] });
    }
  };

  const saveMutation = useMutation({
    mutationFn: (payload: { unitId: string; targetText: string }) =>
      api.patch<TranslationUnit>(`/projects/${projectId}/units/${payload.unitId}`, {
        targetText: payload.targetText,
        status: 'translated',
        comment: '',
      }),
    onSuccess: refreshUnits,
  });

  const reviewMutation = useMutation({
    mutationFn: (unitId: string) => api.post<TranslationUnit>(`/projects/${projectId}/units/${unitId}/review`, { status: 'reviewed', comment: '' }),
    onSuccess: refreshUnits,
  });

  const approveMutation = useMutation({
    mutationFn: (unitId: string) => api.post<TranslationUnit>(`/projects/${projectId}/units/${unitId}/approve`, { status: 'approved', comment: '' }),
    onSuccess: refreshUnits,
  });

  const isDirty = activeUnit ? draftText !== (activeUnit.target?.text || '') : false;
  const isBusy = saveMutation.isPending || reviewMutation.isPending || approveMutation.isPending;

  const handleSave = () => {
    if (!activeUnit || !activeUnit.permissions?.canEdit || !isDirty) {
      return;
    }
    saveMutation.mutate({ unitId: activeUnit.id, targetText: draftText });
  };

  return (
    <div className="flex h-full overflow-hidden bg-slate-100">
      <main className="flex min-w-0 flex-1 bg-slate-50">
        <section className="flex w-[24rem] shrink-0 flex-col border-r border-slate-200 bg-white">
          <div className="border-b border-slate-200 bg-slate-50 px-5 py-4">
            <Link
              to="/project/$projectId/documents"
              params={{ projectId }}
              className="mb-3 inline-flex items-center text-sm font-medium text-blue-600 hover:text-blue-700"
            >
              <ArrowLeft className="mr-1 h-4 w-4" />
              {t('workbench.backToFiles')}
            </Link>
            <div className="text-xs font-semibold uppercase tracking-[0.16em] text-slate-400">{t('workbench.currentFile')}</div>
            <div className="mt-2 truncate text-base font-semibold text-slate-900">
              {selectedDocument ? selectedDocument.title || selectedDocument.path : (isLoadingDocs ? t('workbench.loadingUnits') : t('workbench.selectFile'))}
            </div>
            <div className="mt-4 rounded-2xl border border-slate-200 bg-white px-3 py-2">
              <div className="flex items-center gap-2">
                <select
                  value={searchMode}
                  onChange={(e) => setSearchMode(e.target.value as SearchMode)}
                  className="rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 text-xs font-medium text-slate-600"
                >
                  <option value="source_and_target">{t('workbench.searchScope.sourceAndTarget')}</option>
                  <option value="source_all">{t('workbench.searchScope.sourceAll')}</option>
                  <option value="target">{t('workbench.searchScope.target')}</option>
                </select>
                <Search className="h-4 w-4 text-slate-400" />
                <input
                  value={searchText}
                  onChange={(e) => setSearchText(e.target.value)}
                  placeholder={t('workbench.searchPlaceholder')}
                  className="w-full bg-transparent text-sm outline-none"
                />
                <button
                  type="button"
                  onClick={() => setShowAdvanced((v) => !v)}
                  className={cn(
                    'inline-flex h-9 w-9 items-center justify-center rounded-xl transition',
                    showAdvanced ? 'bg-slate-900 text-white' : 'bg-slate-100 text-slate-500 hover:bg-slate-200',
                  )}
                  title={t('workbench.advanced')}
                >
                  <SlidersHorizontal className="h-4 w-4" />
                </button>
              </div>
            </div>
          </div>

          <div className="flex-1 overflow-y-auto p-3 space-y-2">
            {isLoadingDocs || ((selectedDocId || scope === 'all_documents') && isLoadingUnits) ? (
              <LoadingState label={t('workbench.loadingUnits')} />
            ) : !selectedDocId && scope === 'current_document' ? (
              <EmptyState title={t('workbench.selectFile')} description={t('workbench.selectFileDesc')} />
            ) : units.length === 0 ? (
              <EmptyState title={t('workbench.noUnits')} description={t('workbench.noUnitsDesc')} />
            ) : (
              units.map((unit, index) => {
                const orderedSources = orderSources(unit.sources, preferredSource);
                const primary = orderedSources[0];
                return (
                  <button
                    key={unit.id}
                    type="button"
                    onClick={() => setActiveUnitId(unit.id)}
                    className={cn(
                      'w-full rounded-xl border p-4 text-left transition-all',
                      unit.id === activeUnitId
                        ? 'border-blue-500 bg-blue-50 shadow-sm'
                        : 'border-slate-200 bg-white hover:border-slate-300 hover:bg-slate-50',
                    )}
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div>
                        <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-slate-400">
                          {t('workbench.unit')} {index + 1}
                        </div>
                        {scope === 'all_documents' ? (
                          <div className="mt-1 text-[10px] uppercase tracking-[0.16em] text-slate-400">{unit.document.path}</div>
                        ) : null}
                      </div>
                      <StatusBadge unit={unit} t={t} />
                    </div>
                    <div className="mt-3 line-clamp-3 text-sm leading-6 text-slate-700 whitespace-pre-wrap">
                      {primary?.[1] || t('workbench.noSource')}
                    </div>
                  </button>
                );
              })
            )}
          </div>
        </section>

        <section className="flex min-w-0 flex-1 flex-col">
          <div className="flex items-center justify-between border-b border-slate-200 bg-white px-6 py-4">
            <div className="flex items-center gap-4">
              <span className="text-sm font-medium text-slate-500">
                {activeUnit ? activeUnit.key : t('workbench.selectUnit')}
              </span>
            </div>
            {activeUnit && <StatusBadge unit={activeUnit} large t={t} />}
          </div>

          <div className="flex-1 overflow-y-auto p-6">
            {!selectedDocId && scope === 'current_document' ? (
              <EditorPlaceholder title={t('workbench.chooseFileFirst')} description={t('workbench.chooseFileFirstDesc')} />
            ) : !activeUnit ? (
              <EditorPlaceholder title={t('workbench.selectUnit')} description={t('workbench.chooseUnitDesc')} />
            ) : (
              <div className="mx-auto max-w-5xl space-y-6">
                <section className="rounded-2xl border border-slate-200 bg-white p-6 shadow-sm">
                  <div className="mb-4 flex items-center gap-2 text-sm font-semibold text-slate-900">
                    <FileText className="h-4 w-4 text-blue-600" />
                    {t('workbench.sourceText')}
                  </div>
                  <div className="grid gap-4">
                    {orderSources(activeUnit.sources, preferredSource).map(([language, text], index) => (
                      <SourceBlock
                        key={language}
                        language={language}
                        text={text}
                        emphasized={index === 0}
                        collapsed={index > 0}
                        emptyText={t('workbench.noSourceAvailable')}
                      />
                    ))}
                  </div>
                </section>

                <section className="rounded-2xl border border-slate-200 bg-white p-6 shadow-sm">
                  <div className="mb-4 flex items-center justify-between gap-4">
                    <div>
                      <div className="text-sm font-semibold text-slate-900">{t('workbench.targetTranslation')}</div>
                    </div>
                    {isDirty && (
                      <div className="inline-flex items-center rounded-full bg-amber-100 px-3 py-1 text-sm font-medium text-amber-700">
                        <AlertCircle className="mr-2 h-4 w-4" />
                        {t('workbench.unsavedChanges')}
                      </div>
                    )}
                  </div>

                  <textarea
                    value={draftText}
                    disabled={!activeUnit.permissions?.canEdit || isBusy || activeUnit.isLocked}
                    onChange={(event) => setDraftText(event.target.value)}
                    className={cn(
                      'min-h-[240px] w-full rounded-xl border px-5 py-4 text-base leading-7 outline-none transition-colors resize-y',
                      activeUnit.permissions?.canEdit && !activeUnit.isLocked
                        ? 'border-slate-300 bg-slate-50 focus:border-blue-500 focus:bg-white'
                        : 'cursor-not-allowed border-slate-200 bg-slate-100 text-slate-500',
                    )}
                    placeholder={activeUnit.permissions?.canEdit ? t('workbench.placeholder') : t('workbench.noPermission')}
                  />

                  <div className="mt-6 flex flex-wrap gap-3">
                    <ActionButton
                      onClick={handleSave}
                      disabled={!activeUnit.permissions?.canEdit || !isDirty || isBusy || Boolean(activeUnit.isLocked)}
                      busy={saveMutation.isPending}
                      icon={<Save className="h-4 w-4" />}
                      label={t('workbench.saveTranslation')}
                      tone="primary"
                    />
                    <ActionButton
                      onClick={() => reviewMutation.mutate(activeUnit.id)}
                      disabled={!activeUnit.permissions?.canReview || activeUnit.status === 'reviewed' || activeUnit.status === 'approved' || isBusy}
                      busy={reviewMutation.isPending}
                      icon={<CheckCheck className="h-4 w-4" />}
                      label={t('workbench.markReviewed')}
                      tone="secondary"
                    />
                    <ActionButton
                      onClick={() => approveMutation.mutate(activeUnit.id)}
                      disabled={!activeUnit.permissions?.canApprove || activeUnit.status === 'approved' || isBusy}
                      busy={approveMutation.isPending}
                      icon={<ShieldCheck className="h-4 w-4" />}
                      label={t('workbench.approveUnit')}
                      tone="success"
                    />
                    {(user?.platformRole === 'owner' || user?.platformRole === 'admin') ? (
                      <>
                        <ActionButton
                          onClick={() => api.patch(`/projects/${projectId}/units/${activeUnit.id}`, { isQuestioned: !activeUnit.isQuestioned, targetText: activeUnit.target.text, status: activeUnit.status, comment: activeUnit.comment })}
                          disabled={isBusy}
                          busy={false}
                          icon={<CircleHelp className="h-4 w-4" />}
                          label={activeUnit.isQuestioned ? t('common.cancel') : t('workbench.status.questioned')}
                          tone="secondary"
                        />
                        <ActionButton
                          onClick={() => api.patch(`/projects/${projectId}/units/${activeUnit.id}`, { isLocked: !activeUnit.isLocked, targetText: activeUnit.target.text, status: activeUnit.status, comment: activeUnit.comment })}
                          disabled={isBusy}
                          busy={false}
                          icon={<Lock className="h-4 w-4" />}
                          label={activeUnit.isLocked ? t('common.cancel') : t('workbench.status.locked')}
                          tone="secondary"
                        />
                        <ActionButton
                          onClick={() => api.patch(`/projects/${projectId}/units/${activeUnit.id}`, { isHidden: !activeUnit.isHidden, targetText: activeUnit.target.text, status: activeUnit.status, comment: activeUnit.comment })}
                          disabled={isBusy}
                          busy={false}
                          icon={<EyeOff className="h-4 w-4" />}
                          label={activeUnit.isHidden ? t('common.cancel') : t('workbench.status.hidden')}
                          tone="secondary"
                        />
                      </>
                    ) : null}
                  </div>
                </section>
              </div>
            )}
          </div>
        </section>

        <aside className="w-80 shrink-0 border-l border-slate-200 bg-slate-50">
          <ContextSidebar projectId={projectId} unitId={activeUnitId} />
        </aside>
      </main>

      {showAdvanced ? (
        <div className="absolute inset-0 z-40 flex items-center justify-center bg-slate-950/45 p-6">
          <div className="w-full max-w-4xl rounded-[28px] border border-slate-200 bg-white shadow-2xl">
            <div className="flex items-center justify-between border-b border-slate-200 px-6 py-4">
              <div>
                <div className="text-lg font-semibold text-slate-900">{t('workbench.advanced')}</div>
                <div className="mt-1 text-sm text-slate-500">{t('workbench.advancedDesc')}</div>
              </div>
              <button type="button" onClick={() => setShowAdvanced(false)} className="rounded-full bg-slate-100 p-2 text-slate-500 hover:bg-slate-200">
                <X className="h-4 w-4" />
              </button>
            </div>

            <div className="space-y-5 px-6 py-6">
              <div className="grid gap-4 md:grid-cols-3">
                <label className="space-y-2 text-sm font-medium text-slate-700">
                  <span>{t('workbench.advancedSearchInCurrent')}</span>
                  <button type="button" onClick={() => setScope((value) => value === 'current_document' ? 'all_documents' : 'current_document')} className={cn('flex h-11 items-center justify-between rounded-2xl border px-4', scope === 'current_document' ? 'border-blue-200 bg-blue-50 text-blue-700' : 'border-slate-200 bg-slate-50 text-slate-500')}>
                    <span>{scope === 'current_document' ? t('common.save') : t('common.cancel')}</span>
                    <span>{scope === 'current_document' ? t('workbench.currentDocument') : t('workbench.allDocuments')}</span>
                  </button>
                </label>
                <label className="space-y-2 text-sm font-medium text-slate-700">
                  <span>{t('documents.title')}</span>
                  <input disabled={scope === 'current_document'} value={scope === 'current_document' ? (selectedDocument?.path || '') : ''} placeholder={selectedDocument?.path || ''} className="h-11 w-full rounded-2xl border border-slate-200 bg-slate-50 px-4 text-sm text-slate-500 outline-none disabled:opacity-100" />
                </label>
                <label className="space-y-2 text-sm font-medium text-slate-700">
                  <span>{t('history.allStatuses')}</span>
                  <div className="flex flex-wrap gap-2 rounded-2xl border border-slate-200 bg-slate-50 p-3">
                    {quickStatuses.map((item) => (
                      <button key={item} type="button" onClick={() => setStatusFilters((current) => current.includes(item) ? current.filter((value) => value !== item) : [...current, item])} className={cn('rounded-full px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.16em]', statusFilters.includes(item) ? 'bg-slate-900 text-white' : 'bg-white text-slate-500 border border-slate-200')}>
                        {t(`workbench.status.${item}`)}
                      </button>
                    ))}
                    <button type="button" onClick={() => setOnlyQuestioned((v) => !v)} className={cn('rounded-full px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.16em]', onlyQuestioned ? 'bg-amber-500 text-white' : 'bg-white text-slate-500 border border-slate-200')}>
                      {t('workbench.status.questioned')}
                    </button>
                    <button type="button" onClick={() => setOnlyLocked((v) => !v)} className={cn('rounded-full px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.16em]', onlyLocked ? 'bg-rose-500 text-white' : 'bg-white text-slate-500 border border-slate-200')}>
                      {t('workbench.status.locked')}
                    </button>
                  </div>
                </label>
              </div>

              <div className="space-y-3">
                {advancedConditions.map((condition, index) => (
                  <div key={index} className="grid gap-3 md:grid-cols-[1.1fr_1fr_1.6fr_auto]">
                    <select value={condition.field} onChange={(e) => setAdvancedConditions((items) => items.map((item, i) => i === index ? { ...item, field: e.target.value } : item))} className="rounded-2xl border border-slate-300 px-4 py-3 text-sm">
                      <option value="target">{t('workbench.searchScope.target')}</option>
                      <option value="key">key</option>
                      <option value={`source:${preferredSource}`}>{`${t('workbench.searchScope.sourceAll')}: ${preferredSource}`}</option>
                      <option value="source:en">source:en</option>
                      <option value="source:ja">source:ja</option>
                      <option value="source:ko">source:ko</option>
                    </select>
                    <select value={condition.operator} onChange={(e) => setAdvancedConditions((items) => items.map((item, i) => i === index ? { ...item, operator: e.target.value } : item))} className="rounded-2xl border border-slate-300 px-4 py-3 text-sm">
                      <option value="contains">{t('workbench.operators.contains')}</option>
                      <option value="equals">{t('workbench.operators.equals')}</option>
                      <option value="starts_with">{t('workbench.operators.startsWith')}</option>
                    </select>
                    <input value={condition.value} onChange={(e) => setAdvancedConditions((items) => items.map((item, i) => i === index ? { ...item, value: e.target.value } : item))} className="rounded-2xl border border-slate-300 px-4 py-3 text-sm" />
                    <button type="button" onClick={() => setAdvancedConditions((items) => items.length === 1 ? items : items.filter((_, i) => i !== index))} className="rounded-2xl bg-slate-100 px-4 py-3 text-sm text-slate-600">×</button>
                  </div>
                ))}
                <button type="button" onClick={() => setAdvancedConditions((items) => [...items, { field: 'target', operator: 'contains', value: '' }])} className="rounded-full bg-slate-900 px-4 py-2 text-xs font-semibold uppercase tracking-[0.16em] text-white">
                  + {t('workbench.advanced')}
                </button>
              </div>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}

function orderSources(sources: Record<string, string>, preferredSource: string) {
  const entries = Object.entries(sources || {});
  entries.sort(([a], [b]) => {
    if (a === preferredSource) return -1;
    if (b === preferredSource) return 1;
    return a.localeCompare(b);
  });
  return entries;
}

function normalizePreferredSource(value: string) {
  return value === 'jp' ? 'ja' : value;
}

function SourceBlock({ language, text, emphasized = false, collapsed = false, emptyText }: { language: string; text: string; emphasized?: boolean; collapsed?: boolean; emptyText: string }) {
  const [open, setOpen] = useState(!collapsed);

  return (
    <div className={cn('rounded-xl border p-4', emphasized ? 'border-blue-200 bg-blue-50/70' : 'border-slate-200 bg-slate-50')}>
      <button type="button" onClick={() => setOpen((v) => !v)} className="flex w-full items-center justify-between text-left">
        <div className="text-[11px] font-semibold uppercase tracking-[0.2em] text-slate-400">{language}</div>
        {!emphasized ? <span className="text-[10px] text-slate-400">{open ? '−' : '+'}</span> : null}
      </button>
      {open ? <div className="mt-3 whitespace-pre-wrap text-base leading-7 text-slate-800">{text || emptyText}</div> : null}
    </div>
  );
}

function StatusBadge({ unit, large = false, t }: { unit: TranslationUnit; large?: boolean; t: (key: string) => string }) {
  const normalized = unit.status || 'untranslated';

  const getStatusText = (s: string) => {
    switch (s) {
      case 'approved': return t('workbench.status.approved');
      case 'reviewed': return t('workbench.status.reviewed');
      case 'translated': return t('workbench.status.translated');
      default: return t('workbench.status.untranslated');
    }
  };

  return (
    <div className="flex flex-wrap items-center justify-end gap-2">
      {unit.isQuestioned ? (
        <span className="inline-flex items-center rounded-md bg-amber-100 px-2 py-1 text-[10px] font-semibold uppercase tracking-[0.16em] text-amber-700">
          <CircleHelp className="mr-1 h-3 w-3" />
          {t('workbench.status.questioned')}
        </span>
      ) : null}
      {unit.isLocked ? (
        <span className="inline-flex items-center rounded-md bg-rose-100 px-2 py-1 text-[10px] font-semibold uppercase tracking-[0.16em] text-rose-700">
          <Lock className="mr-1 h-3 w-3" />
          {t('workbench.status.locked')}
        </span>
      ) : null}
      {unit.isHidden ? (
        <span className="inline-flex items-center rounded-md bg-slate-200 px-2 py-1 text-[10px] font-semibold uppercase tracking-[0.16em] text-slate-700">
          <EyeOff className="mr-1 h-3 w-3" />
          {t('workbench.status.hidden')}
        </span>
      ) : null}
      <span
        className={cn(
          'inline-flex items-center rounded-md font-semibold uppercase tracking-[0.16em]',
          large ? 'px-3 py-1.5 text-xs' : 'px-2 py-1 text-[10px]',
          normalized === 'approved'
            ? 'bg-emerald-100 text-emerald-700'
            : normalized === 'reviewed'
              ? 'bg-blue-100 text-blue-700'
              : normalized === 'translated'
                ? 'bg-violet-100 text-violet-700'
                : 'bg-slate-100 text-slate-500',
        )}
      >
        {getStatusText(normalized)}
      </span>
    </div>
  );
}

function ActionButton({
  onClick,
  disabled,
  busy,
  icon,
  label,
  tone,
}: {
  onClick: () => void;
  disabled: boolean;
  busy: boolean;
  icon: React.ReactNode;
  label: string;
  tone: 'primary' | 'secondary' | 'success';
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={cn(
        'inline-flex min-w-[140px] items-center justify-center rounded-lg px-4 py-2.5 text-sm font-medium transition disabled:cursor-not-allowed disabled:opacity-50',
        tone === 'primary' && 'bg-blue-600 text-white hover:bg-blue-700 shadow-sm',
        tone === 'secondary' && 'bg-white border border-slate-300 text-slate-700 hover:bg-slate-50 shadow-sm',
        tone === 'success' && 'bg-emerald-600 text-white hover:bg-emerald-700 shadow-sm',
      )}
    >
      {busy ? <LoaderCircle className="mr-2 h-4 w-4 animate-spin" /> : <span className="mr-2">{icon}</span>}
      {label}
    </button>
  );
}

function LoadingState({ label }: { label: string }) {
  return (
    <div className="flex h-full min-h-[160px] items-center justify-center text-sm text-slate-500">
      <LoaderCircle className="mr-2 h-4 w-4 animate-spin" />
      {label}
    </div>
  );
}

function EmptyState({ title, description }: { title: string; description: string }) {
  return (
    <div className="rounded-xl border border-dashed border-slate-200 bg-slate-50 px-4 py-6 text-center">
      <div className="text-sm font-semibold text-slate-700">{title}</div>
      <p className="mt-2 text-xs leading-6 text-slate-500">{description}</p>
    </div>
  );
}

function EditorPlaceholder({ title, description }: { title: string; description: string }) {
  return (
    <div className="flex h-full items-center justify-center">
      <div className="max-w-md rounded-2xl border border-dashed border-slate-300 bg-white px-8 py-10 text-center shadow-sm">
        <div className="text-lg font-semibold text-slate-900">{title}</div>
        <p className="mt-2 text-sm leading-6 text-slate-500">{description}</p>
      </div>
    </div>
  );
}

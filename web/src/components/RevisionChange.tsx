import { ArrowRight } from 'lucide-react';

import { cn } from '@/lib/utils';

export interface RevisionChangeItem {
  beforeTargetText: string;
  afterTargetText: string;
  beforeStatus: string;
  afterStatus: string;
}

export function RevisionChange({
  item,
  t,
  compact = false,
}: {
  item: RevisionChangeItem;
  t: (key: string) => string;
  compact?: boolean;
}) {
  const beforeText = item.beforeTargetText || '';
  const afterText = item.afterTargetText || '';
  const hasTextChange = beforeText !== afterText;
  const diff = buildTextDiff(beforeText, afterText);

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2 text-xs text-slate-500">
        <StatusChip status={item.beforeStatus} t={t} tone="before" />
        <ArrowRight className="h-3.5 w-3.5 text-slate-300" />
        <StatusChip status={item.afterStatus} t={t} tone="after" />
      </div>

      {hasTextChange ? (
        <div className={cn('grid gap-3', compact ? 'grid-cols-1' : 'md:grid-cols-2')}>
          <DiffPanel
            label={t('history.before')}
            tone="before"
            segments={diff.beforeSegments}
            compact={compact}
          />
          <DiffPanel
            label={t('history.after')}
            tone="after"
            segments={diff.afterSegments}
            compact={compact}
          />
        </div>
      ) : null}
    </div>
  );
}

function DiffPanel({
  label,
  tone,
  segments,
  compact,
}: {
  label: string;
  tone: 'before' | 'after';
  segments: DiffSegment[];
  compact: boolean;
}) {
  return (
    <div
      className={cn(
        'rounded-2xl p-4',
        compact
          ? tone === 'before'
            ? 'bg-slate-50'
            : 'bg-blue-50'
          : tone === 'before'
            ? 'bg-slate-50'
            : 'bg-emerald-50',
      )}
    >
      <div
        className={cn(
          'text-xs font-semibold uppercase tracking-[0.18em]',
          tone === 'before' ? 'text-slate-400' : compact ? 'text-blue-500' : 'text-emerald-500',
        )}
      >
        {label}
      </div>
      <div className="mt-3 whitespace-pre-wrap text-sm leading-6 text-slate-700">
        {segments.length > 0 ? (
          segments.map((segment, index) => (
            <span
              key={index}
              className={cn(
                segment.changed && tone === 'before' && 'rounded bg-rose-100/90 text-rose-700',
                segment.changed && tone === 'after' && 'rounded bg-emerald-100/90 text-emerald-700',
              )}
            >
              {segment.text}
            </span>
          ))
        ) : (
          <span className="text-slate-400">∅</span>
        )}
      </div>
    </div>
  );
}

function StatusChip({
  status,
  t,
  tone,
}: {
  status: string;
  t: (key: string) => string;
  tone: 'before' | 'after';
}) {
  return (
    <span
      className={cn(
        'inline-flex items-center rounded-full px-2.5 py-1 text-[11px] font-semibold',
        tone === 'before' ? 'bg-slate-100 text-slate-600' : 'bg-blue-100 text-blue-700',
      )}
    >
      {translateStatus(status, t)}
    </span>
  );
}

type DiffSegment = {
  text: string;
  changed: boolean;
};

function buildTextDiff(before: string, after: string) {
  if (before === after) {
    return {
      beforeSegments: before ? [{ text: before, changed: false }] : [],
      afterSegments: after ? [{ text: after, changed: false }] : [],
    };
  }

  let prefixLength = 0;
  while (
    prefixLength < before.length &&
    prefixLength < after.length &&
    before[prefixLength] === after[prefixLength]
  ) {
    prefixLength += 1;
  }

  let beforeEnd = before.length - 1;
  let afterEnd = after.length - 1;
  while (
    beforeEnd >= prefixLength &&
    afterEnd >= prefixLength &&
    before[beforeEnd] === after[afterEnd]
  ) {
    beforeEnd -= 1;
    afterEnd -= 1;
  }

  return {
    beforeSegments: compactSegments([
      { text: before.slice(0, prefixLength), changed: false },
      { text: before.slice(prefixLength, beforeEnd + 1), changed: true },
      { text: before.slice(beforeEnd + 1), changed: false },
    ]),
    afterSegments: compactSegments([
      { text: after.slice(0, prefixLength), changed: false },
      { text: after.slice(prefixLength, afterEnd + 1), changed: true },
      { text: after.slice(afterEnd + 1), changed: false },
    ]),
  };
}

function compactSegments(segments: DiffSegment[]) {
  return segments.filter((segment) => segment.text.length > 0);
}

function translateStatus(status: string, t: (key: string) => string) {
  switch (status) {
    case 'approved':
    case 'reviewed':
    case 'translated':
    case 'untranslated':
    case 'questioned':
    case 'locked':
    case 'hidden':
      return t(`workbench.status.${status}`);
    default:
      return status || t('workbench.status.untranslated');
  }
}

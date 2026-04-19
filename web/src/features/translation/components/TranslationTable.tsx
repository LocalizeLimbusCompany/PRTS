import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api, PaginatedData } from '@/api/client';
import { useAuthStore } from '@/store/auth';
import { Check, CheckCircle2, CircleDashed, ChevronDown, ChevronRight, Save } from 'lucide-react';
import { cn } from '@/lib/utils';

interface Props {
  projectId: string;
  documentId: string | null;
  page: number;
  onPageChange: (page: number) => void;
}

export default function TranslationTable({ projectId, documentId, page, onPageChange }: Props) {
  const queryClient = useQueryClient();
  const user = useAuthStore(s => s.user);
  const preferredSource = user?.preferredSourceLanguage || 'en';

  const { data, isLoading } = useQuery({
    queryKey: ['units', projectId, documentId, page],
    queryFn: () => {
      const params = new URLSearchParams({ page: page.toString(), pageSize: '50' });
      if (documentId) params.append('documentId', documentId);
      return api.get<PaginatedData<any>>(`/projects/${projectId}/units?${params}`);
    },
  });

  const updateUnit = useMutation({
    mutationFn: (args: { unitId: string; targetText: string; status: string }) => 
      api.patch(`/projects/${projectId}/units/${args.unitId}`, { 
        targetText: args.targetText, 
        status: args.status 
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['units', projectId, documentId, page] });
    }
  });

  const reviewUnit = useMutation({
    mutationFn: (args: { unitId: string }) => 
      api.post(`/projects/${projectId}/units/${args.unitId}/review`, { status: 'reviewed', comment: '' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['units', projectId, documentId, page] })
  });

  const approveUnit = useMutation({
    mutationFn: (args: { unitId: string }) => 
      api.post(`/projects/${projectId}/units/${args.unitId}/approve`, { status: 'approved', comment: '' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['units', projectId, documentId, page] })
  });

  if (isLoading) return <div className="p-4 text-slate-500">Loading units...</div>;
  if (!data?.items || data.items.length === 0) return <div className="p-4 text-slate-500">No translation units found.</div>;

  return (
    <div className="flex flex-col h-full">
      <div className="flex-1 overflow-y-auto bg-slate-50">
        <table className="w-full text-left border-collapse">
          <thead className="bg-slate-100 sticky top-0 z-10 shadow-sm">
            <tr>
              <th className="w-8 p-2 border-b border-slate-200"></th>
              <th className="w-1/3 p-2 font-medium text-xs text-slate-500 border-b border-slate-200 uppercase tracking-wider">Source ({preferredSource})</th>
              <th className="w-1/2 p-2 font-medium text-xs text-slate-500 border-b border-slate-200 uppercase tracking-wider">Target</th>
              <th className="w-32 p-2 font-medium text-xs text-slate-500 border-b border-slate-200 uppercase tracking-wider">Status</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-200 bg-white">
            {data.items.map(unit => (
              <UnitRow 
                key={unit.id} 
                unit={unit} 
                preferredSource={preferredSource}
                onSave={(text) => updateUnit.mutate({ unitId: unit.id, targetText: text, status: 'translated' })}
                onReview={() => reviewUnit.mutate({ unitId: unit.id })}
                onApprove={() => approveUnit.mutate({ unitId: unit.id })}
              />
            ))}
          </tbody>
        </table>
      </div>
      
      {/* Pagination */}
      <div className="p-3 border-t border-slate-200 bg-white flex justify-between items-center text-sm shrink-0">
        <div className="text-slate-500">
          Showing {(page - 1) * data.pageSize + 1} to {Math.min(page * data.pageSize, data.total)} of {data.total}
        </div>
        <div className="flex gap-2">
          <button 
            disabled={page === 1}
            onClick={() => onPageChange(page - 1)}
            className="px-3 py-1 border border-slate-300 rounded hover:bg-slate-50 disabled:opacity-50"
          >
            Previous
          </button>
          <button 
            disabled={page * data.pageSize >= data.total}
            onClick={() => onPageChange(page + 1)}
            className="px-3 py-1 border border-slate-300 rounded hover:bg-slate-50 disabled:opacity-50"
          >
            Next
          </button>
        </div>
      </div>
    </div>
  );
}

function UnitRow({ 
  unit, 
  preferredSource, 
  onSave, 
  onReview, 
  onApprove 
}: { 
  unit: any, 
  preferredSource: string, 
  onSave: (t: string) => void,
  onReview: () => void,
  onApprove: () => void
}) {
  const [isExpanded, setIsExpanded] = useState(false);
  const [targetText, setTargetText] = useState(unit.target?.text || '');
  const [isDirty, setIsDirty] = useState(false);

  const sources = unit.sources || {};
  const prefText = sources[preferredSource] || Object.values(sources)[0] || '';
  const otherLangs = Object.keys(sources).filter(l => l !== preferredSource);

  const canEdit = unit.permissions?.canEdit !== false;
  const canReview = unit.permissions?.canReview === true;
  const canApprove = unit.permissions?.canApprove === true;

  const handleBlur = () => {
    if (isDirty && targetText !== unit.target?.text && canEdit) {
      onSave(targetText);
      setIsDirty(false);
    }
  };

  return (
    <tr className="hover:bg-slate-50/50 group align-top">
      <td className="p-2 pl-3">
        {unit.status === 'approved' ? (
          <CheckCircle2 className="w-4 h-4 text-green-500" />
        ) : unit.status === 'translated' || unit.status === 'reviewed' ? (
          <Check className="w-4 h-4 text-blue-500" />
        ) : (
          <CircleDashed className="w-4 h-4 text-slate-300" />
        )}
      </td>
      <td className="p-2 border-r border-slate-100">
        <div className="text-[10px] font-mono text-slate-400 mb-1 select-all">{unit.key}</div>
        <div className="text-sm text-slate-800 whitespace-pre-wrap">{prefText}</div>
        
        {otherLangs.length > 0 && (
          <div className="mt-2">
            <button 
              onClick={() => setIsExpanded(!isExpanded)}
              className="text-xs text-slate-400 hover:text-blue-500 flex items-center transition-colors"
            >
              {isExpanded ? <ChevronDown className="w-3 h-3 mr-1" /> : <ChevronRight className="w-3 h-3 mr-1" />}
              {otherLangs.length} other languages
            </button>
            
            {isExpanded && (
              <div className="mt-2 space-y-2 pl-4 border-l-2 border-slate-100">
                {otherLangs.map(lang => (
                  <div key={lang}>
                    <div className="text-[10px] font-bold text-slate-400 uppercase">{lang}</div>
                    <div className="text-xs text-slate-600 whitespace-pre-wrap">{sources[lang]}</div>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}
      </td>
      <td className="p-0 border-r border-slate-100 relative bg-white">
        <textarea
          disabled={!canEdit}
          className={cn(
            "w-full h-full min-h-[60px] p-2 text-sm resize-y outline-none transition-colors",
            !canEdit ? "bg-slate-50 text-slate-500 cursor-not-allowed" : 
            isDirty ? "bg-yellow-50/30" : "focus:bg-blue-50/30"
          )}
          value={targetText}
          onChange={(e) => {
            setTargetText(e.target.value);
            setIsDirty(true);
          }}
          onBlur={handleBlur}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
              e.preventDefault();
              e.currentTarget.blur();
            }
          }}
          placeholder={canEdit ? "Enter translation here... (Ctrl+Enter to save)" : "No permission to edit"}
        />
        {isDirty && canEdit && (
          <div className="absolute bottom-1 right-1 text-[10px] text-yellow-600 flex items-center bg-yellow-100 px-1.5 py-0.5 rounded">
            <Save className="w-3 h-3 mr-1" /> Unsaved
          </div>
        )}
      </td>
      <td className="p-2 text-xs">
        <span className={cn(
          "inline-flex items-center px-2 py-0.5 rounded text-[11px] font-medium uppercase tracking-wider mb-2",
          unit.status === 'approved' ? "bg-green-100 text-green-700" :
          unit.status === 'reviewed' ? "bg-blue-100 text-blue-700" :
          unit.status === 'translated' ? "bg-slate-100 text-slate-700" :
          "bg-slate-50 text-slate-400 border border-slate-200"
        )}>
          {unit.status || 'Untranslated'}
        </span>
        <div className="text-[10px] text-slate-400 mb-2">
          {unit.updatedBy ? `By ${unit.updatedBy.name}` : ''}
        </div>
        <div className="flex flex-col gap-1 mt-1">
          {canReview && unit.status !== 'reviewed' && unit.status !== 'approved' && (
             <button 
               onClick={onReview}
               className="px-2 py-1 bg-blue-50 text-blue-600 rounded hover:bg-blue-100 text-left transition-colors"
             >
               Mark as Reviewed
             </button>
          )}
          {canApprove && unit.status !== 'approved' && (
             <button 
               onClick={onApprove}
               className="px-2 py-1 bg-green-50 text-green-600 rounded hover:bg-green-100 text-left transition-colors"
             >
               Approve
             </button>
          )}
        </div>
      </td>
    </tr>
  );
}

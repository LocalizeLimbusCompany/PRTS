import { createFileRoute, redirect } from '@tanstack/react-router';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { LoaderCircle, Plus, Trash2, Edit2, BookA, Save } from 'lucide-react';
import { getGlossaryTerms, createGlossaryTerm, updateGlossaryTerm, deleteGlossaryTerm, GlossaryTerm } from '@/api/glossary';
import { useTranslation } from '@/hooks/useTranslation';

export const Route = createFileRoute('/project/$projectId/glossary')({
  beforeLoad: () => {
    if (!localStorage.getItem('auth-storage') || !JSON.parse(localStorage.getItem('auth-storage') || '{}').state?.token) {
      throw redirect({ to: '/login' });
    }
  },
  component: GlossaryPage,
});

function GlossaryPage() {
  const { projectId } = Route.useParams();
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  const [isAdding, setIsAdding] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);

  // Form state
  const [sourceTerm, setSourceTerm] = useState('');
  const [targetTerm, setTargetTerm] = useState('');
  const [sourceLanguage, setSourceLanguage] = useState('en');
  const [targetLanguage, setTargetLanguage] = useState('cn');
  const [note, setNote] = useState('');

  const { data: glossaryData, isLoading } = useQuery({
    queryKey: ['glossary', projectId],
    queryFn: () => getGlossaryTerms(projectId),
  });

  const terms = glossaryData?.items || [];

  const createMutation = useMutation({
    mutationFn: () => createGlossaryTerm(projectId, { sourceTerm, targetTerm, sourceLanguage, targetLanguage, note }),
    onSuccess: () => {
      resetForm();
      queryClient.invalidateQueries({ queryKey: ['glossary', projectId] });
    },
  });

  const updateMutation = useMutation({
    mutationFn: (id: string) => updateGlossaryTerm(projectId, id, { sourceTerm, targetTerm, sourceLanguage, targetLanguage, note }),
    onSuccess: () => {
      resetForm();
      queryClient.invalidateQueries({ queryKey: ['glossary', projectId] });
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => deleteGlossaryTerm(projectId, id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['glossary', projectId] });
    },
  });

  const resetForm = () => {
    setIsAdding(false);
    setEditingId(null);
    setSourceTerm('');
    setTargetTerm('');
    setSourceLanguage('en');
    setTargetLanguage('cn');
    setNote('');
  };

  const startEditing = (term: GlossaryTerm) => {
    setEditingId(term.id);
    setIsAdding(false);
    setSourceTerm(term.sourceTerm);
    setTargetTerm(term.targetTerm);
    setSourceLanguage(term.sourceLanguage);
    setTargetLanguage(term.targetLanguage);
    setNote(term.note);
  };

  const startAdding = () => {
    resetForm();
    setIsAdding(true);
  };

  const handleSave = () => {
    if (!sourceTerm || !targetTerm) return;
    if (editingId) {
      updateMutation.mutate(editingId);
    } else {
      createMutation.mutate();
    }
  };

  return (
    <div className="p-8 h-full overflow-y-auto bg-slate-50">
      <div className="max-w-6xl mx-auto space-y-8">
        <div className="flex justify-between items-center">
          <div>
            <h1 className="text-3xl font-bold text-slate-900 flex items-center">
              <BookA className="w-8 h-8 mr-3 text-blue-600" />
              Glossary
            </h1>
            <p className="mt-2 text-slate-500">{t('projects.subtitle')}</p>
          </div>
          {!isAdding && !editingId && (
            <button
              onClick={startAdding}
              className="px-5 py-2.5 bg-blue-600 text-white rounded-xl font-medium hover:bg-blue-700 transition-colors flex items-center shadow-sm"
            >
              <Plus className="w-5 h-5 mr-2" />
              Add Term
            </button>
          )}
        </div>

        {/* Edit / Add Form */}
        {(isAdding || editingId) && (
          <div className="bg-white p-6 rounded-2xl border border-slate-200 shadow-sm animate-in fade-in slide-in-from-top-4">
            <h2 className="text-lg font-semibold text-slate-900 mb-6">{editingId ? 'Edit Term' : 'Add New Term'}</h2>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-6 mb-6">
              <div>
                <label className="block text-sm font-medium text-slate-700 mb-1">Source Term <span className="text-red-500">*</span></label>
                <div className="flex gap-2">
                  <input
                    type="text"
                    value={sourceLanguage}
                    onChange={e => setSourceLanguage(e.target.value)}
                    className="w-20 rounded-lg border border-slate-300 px-3 py-2 outline-none focus:border-blue-500 font-mono text-sm uppercase text-center"
                    placeholder="Lang"
                  />
                  <input
                    type="text"
                    value={sourceTerm}
                    onChange={e => setSourceTerm(e.target.value)}
                    className="flex-1 rounded-lg border border-slate-300 px-4 py-2 outline-none focus:border-blue-500"
                    placeholder="e.g. Rhodes Island"
                    autoFocus
                  />
                </div>
              </div>
              <div>
                <label className="block text-sm font-medium text-slate-700 mb-1">Target Term <span className="text-red-500">*</span></label>
                <div className="flex gap-2">
                  <input
                    type="text"
                    value={targetLanguage}
                    onChange={e => setTargetLanguage(e.target.value)}
                    className="w-20 rounded-lg border border-slate-300 px-3 py-2 outline-none focus:border-blue-500 font-mono text-sm uppercase text-center"
                    placeholder="Lang"
                  />
                  <input
                    type="text"
                    value={targetTerm}
                    onChange={e => setTargetTerm(e.target.value)}
                    className="flex-1 rounded-lg border border-slate-300 px-4 py-2 outline-none focus:border-blue-500"
                    placeholder="e.g. 罗德岛"
                  />
                </div>
              </div>
              <div className="md:col-span-2">
                <label className="block text-sm font-medium text-slate-700 mb-1">Notes (Optional)</label>
                <textarea
                  value={note}
                  onChange={e => setNote(e.target.value)}
                  className="w-full rounded-lg border border-slate-300 px-4 py-3 outline-none focus:border-blue-500 min-h-[80px]"
                  placeholder="Additional context or usage instructions"
                />
              </div>
            </div>
            <div className="flex justify-end gap-3 pt-4 border-t border-slate-100">
              <button
                onClick={resetForm}
                className="px-5 py-2 text-slate-700 bg-white border border-slate-300 rounded-lg font-medium hover:bg-slate-50 transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={handleSave}
                disabled={!sourceTerm || !targetTerm || createMutation.isPending || updateMutation.isPending}
                className="px-6 py-2 bg-blue-600 text-white rounded-lg font-medium hover:bg-blue-700 disabled:opacity-50 transition-colors flex items-center"
              >
                {(createMutation.isPending || updateMutation.isPending) ? <LoaderCircle className="w-5 h-5 animate-spin mr-2" /> : <Save className="w-5 h-5 mr-2" />}
                Save Term
              </button>
            </div>
          </div>
        )}

        {/* List */}
        <div className="bg-white rounded-2xl border border-slate-200 shadow-sm overflow-hidden">
          {isLoading ? (
            <div className="flex justify-center items-center py-20 text-slate-500">
              <LoaderCircle className="w-6 h-6 animate-spin mr-2" />
              Loading glossary...
            </div>
          ) : terms.length === 0 ? (
            <div className="py-20 text-center text-slate-500">
              <BookA className="w-12 h-12 mx-auto mb-4 text-slate-300" />
              <p className="text-lg font-medium text-slate-900">Glossary is empty</p>
              <p className="mt-1">Add terms to build your project's dictionary.</p>
            </div>
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full text-left border-collapse">
                <thead>
                  <tr className="bg-slate-50 border-b border-slate-200">
                    <th className="px-6 py-4 font-semibold text-slate-900 w-1/3">Source Term</th>
                    <th className="px-6 py-4 font-semibold text-slate-900 w-1/3">Target Term</th>
                    <th className="px-6 py-4 font-semibold text-slate-900 w-auto">Notes</th>
                    <th className="px-6 py-4 font-semibold text-slate-900 w-[100px] text-right">Actions</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-slate-100">
                  {terms.map(term => (
                    <tr key={term.id} className="hover:bg-slate-50/50 transition-colors">
                      <td className="px-6 py-4">
                        <div className="flex flex-col">
                          <span className="text-xs font-bold uppercase text-slate-400 mb-1 tracking-wider">{term.sourceLanguage}</span>
                          <span className="text-base font-medium text-slate-900">{term.sourceTerm}</span>
                        </div>
                      </td>
                      <td className="px-6 py-4">
                        <div className="flex flex-col">
                          <span className="text-xs font-bold uppercase text-slate-400 mb-1 tracking-wider">{term.targetLanguage}</span>
                          <span className="text-base font-medium text-blue-700">{term.targetTerm}</span>
                        </div>
                      </td>
                      <td className="px-6 py-4">
                        <span className="text-sm text-slate-600 line-clamp-2">{term.note || '-'}</span>
                      </td>
                      <td className="px-6 py-4 text-right">
                        <div className="flex justify-end gap-2">
                          <button
                            onClick={() => startEditing(term)}
                            className="p-2 text-slate-400 hover:text-blue-600 hover:bg-blue-50 rounded-lg transition-colors"
                            title="Edit"
                          >
                            <Edit2 className="w-4 h-4" />
                          </button>
                          <button
                            onClick={() => {
                              if (confirm('Are you sure you want to delete this term?')) {
                                deleteMutation.mutate(term.id);
                              }
                            }}
                            className="p-2 text-slate-400 hover:text-red-600 hover:bg-red-50 rounded-lg transition-colors"
                            title="Delete"
                          >
                            <Trash2 className="w-4 h-4" />
                          </button>
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

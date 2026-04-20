import { createFileRoute, redirect } from '@tanstack/react-router';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { LoaderCircle, Plus, Trash2, Edit2, Database, Save } from 'lucide-react';
import { getTMEntries, createTMEntry, updateTMEntry, deleteTMEntry, TMEntry } from '@/api/tm';

export const Route = createFileRoute('/project/$projectId/tm')({
  beforeLoad: () => {
    if (!localStorage.getItem('auth-storage') || !JSON.parse(localStorage.getItem('auth-storage') || '{}').state?.token) {
      throw redirect({ to: '/login' });
    }
  },
  component: TMPage,
});

function TMPage() {
  const { projectId } = Route.useParams();
  const queryClient = useQueryClient();

  const [isAdding, setIsAdding] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);

  // Form state
  const [sourceLanguage, setSourceLanguage] = useState('en');
  const [targetLanguage, setTargetLanguage] = useState('cn');
  const [sourceText, setSourceText] = useState('');
  const [targetText, setTargetText] = useState('');
  const [qualityScore, setQualityScore] = useState<number>(100);

  const { data: tmData, isLoading } = useQuery({
    queryKey: ['tm', projectId],
    queryFn: () => getTMEntries(projectId),
  });

  const entries = tmData?.items || [];

  const createMutation = useMutation({
    mutationFn: () => createTMEntry(projectId, { sourceLanguage, targetLanguage, sourceText, targetText, qualityScore }),
    onSuccess: () => {
      resetForm();
      queryClient.invalidateQueries({ queryKey: ['tm', projectId] });
    },
  });

  const updateMutation = useMutation({
    mutationFn: (id: string) => updateTMEntry(projectId, id, { sourceLanguage, targetLanguage, sourceText, targetText, qualityScore }),
    onSuccess: () => {
      resetForm();
      queryClient.invalidateQueries({ queryKey: ['tm', projectId] });
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => deleteTMEntry(projectId, id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['tm', projectId] });
    },
  });

  const resetForm = () => {
    setIsAdding(false);
    setEditingId(null);
    setSourceLanguage('en');
    setTargetLanguage('cn');
    setSourceText('');
    setTargetText('');
    setQualityScore(100);
  };

  const startEditing = (entry: TMEntry) => {
    setEditingId(entry.id);
    setIsAdding(false);
    setSourceLanguage(entry.sourceLanguage);
    setTargetLanguage(entry.targetLanguage);
    setSourceText(entry.sourceText);
    setTargetText(entry.targetText);
    setQualityScore(entry.qualityScore);
  };

  const startAdding = () => {
    resetForm();
    setIsAdding(true);
  };

  const handleSave = () => {
    if (!sourceText || !targetText) return;
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
              <Database className="w-8 h-8 mr-3 text-blue-600" />
              Translation Memory
            </h1>
            <p className="mt-2 text-slate-500">Manage translation memory entries to speed up translation process.</p>
          </div>
          {!isAdding && !editingId && (
            <button
              onClick={startAdding}
              className="px-5 py-2.5 bg-blue-600 text-white rounded-xl font-medium hover:bg-blue-700 transition-colors flex items-center shadow-sm"
            >
              <Plus className="w-5 h-5 mr-2" />
              Add Entry
            </button>
          )}
        </div>

        {/* Edit / Add Form */}
        {(isAdding || editingId) && (
          <div className="bg-white p-6 rounded-2xl border border-slate-200 shadow-sm animate-in fade-in slide-in-from-top-4">
            <h2 className="text-lg font-semibold text-slate-900 mb-6">{editingId ? 'Edit TM Entry' : 'Add New TM Entry'}</h2>
            
            <div className="grid grid-cols-1 md:grid-cols-2 gap-6 mb-6">
              <div className="md:col-span-2 grid grid-cols-2 gap-6">
                 <div>
                   <label className="block text-sm font-medium text-slate-700 mb-1">Source Language</label>
                   <input
                     type="text"
                     value={sourceLanguage}
                     onChange={e => setSourceLanguage(e.target.value)}
                     className="w-full rounded-lg border border-slate-300 px-4 py-2 outline-none focus:border-blue-500 uppercase"
                   />
                 </div>
                 <div>
                   <label className="block text-sm font-medium text-slate-700 mb-1">Target Language</label>
                   <input
                     type="text"
                     value={targetLanguage}
                     onChange={e => setTargetLanguage(e.target.value)}
                     className="w-full rounded-lg border border-slate-300 px-4 py-2 outline-none focus:border-blue-500 uppercase"
                   />
                 </div>
              </div>
              
              <div className="md:col-span-2">
                <label className="block text-sm font-medium text-slate-700 mb-1">Source Text <span className="text-red-500">*</span></label>
                <textarea
                  value={sourceText}
                  onChange={e => setSourceText(e.target.value)}
                  className="w-full rounded-lg border border-slate-300 px-4 py-3 outline-none focus:border-blue-500 min-h-[100px]"
                  placeholder="e.g. Doctor, wake up."
                  autoFocus
                />
              </div>

              <div className="md:col-span-2">
                <label className="block text-sm font-medium text-slate-700 mb-1">Target Text <span className="text-red-500">*</span></label>
                <textarea
                  value={targetText}
                  onChange={e => setTargetText(e.target.value)}
                  className="w-full rounded-lg border border-slate-300 px-4 py-3 outline-none focus:border-blue-500 min-h-[100px]"
                  placeholder="e.g. 博士，醒醒。"
                />
              </div>
              
              <div className="md:col-span-2 w-1/3">
                <label className="block text-sm font-medium text-slate-700 mb-1">Quality Score (0-100)</label>
                <input
                  type="number"
                  min="0"
                  max="100"
                  value={qualityScore}
                  onChange={e => setQualityScore(parseInt(e.target.value) || 0)}
                  className="w-full rounded-lg border border-slate-300 px-4 py-2 outline-none focus:border-blue-500"
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
                disabled={!sourceText || !targetText || createMutation.isPending || updateMutation.isPending}
                className="px-6 py-2 bg-blue-600 text-white rounded-lg font-medium hover:bg-blue-700 disabled:opacity-50 transition-colors flex items-center"
              >
                {(createMutation.isPending || updateMutation.isPending) ? <LoaderCircle className="w-5 h-5 animate-spin mr-2" /> : <Save className="w-5 h-5 mr-2" />}
                Save Entry
              </button>
            </div>
          </div>
        )}

        {/* List */}
        <div className="bg-white rounded-2xl border border-slate-200 shadow-sm overflow-hidden">
          {isLoading ? (
            <div className="flex justify-center items-center py-20 text-slate-500">
              <LoaderCircle className="w-6 h-6 animate-spin mr-2" />
              Loading translation memory...
            </div>
          ) : entries.length === 0 ? (
            <div className="py-20 text-center text-slate-500">
              <Database className="w-12 h-12 mx-auto mb-4 text-slate-300" />
              <p className="text-lg font-medium text-slate-900">Translation memory is empty</p>
              <p className="mt-1">Add entries to reuse past translations.</p>
            </div>
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full text-left border-collapse">
                <thead>
                  <tr className="bg-slate-50 border-b border-slate-200">
                    <th className="px-6 py-4 font-semibold text-slate-900 w-[40%]">Source Text</th>
                    <th className="px-6 py-4 font-semibold text-slate-900 w-[40%]">Target Text</th>
                    <th className="px-6 py-4 font-semibold text-slate-900 text-center w-auto">Quality</th>
                    <th className="px-6 py-4 font-semibold text-slate-900 w-[100px] text-right">Actions</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-slate-100">
                  {entries.map(entry => (
                    <tr key={entry.id} className="hover:bg-slate-50/50 transition-colors align-top">
                      <td className="px-6 py-4">
                        <div className="flex flex-col">
                          <span className="text-xs font-bold uppercase text-slate-400 mb-1 tracking-wider">{entry.sourceLanguage}</span>
                          <span className="text-base text-slate-900 whitespace-pre-wrap">{entry.sourceText}</span>
                        </div>
                      </td>
                      <td className="px-6 py-4">
                        <div className="flex flex-col">
                          <span className="text-xs font-bold uppercase text-slate-400 mb-1 tracking-wider">{entry.targetLanguage}</span>
                          <span className="text-base text-blue-700 whitespace-pre-wrap">{entry.targetText}</span>
                        </div>
                      </td>
                      <td className="px-6 py-4 text-center">
                        <span className={`inline-flex items-center justify-center rounded-full px-2.5 py-1 text-xs font-semibold ${entry.qualityScore >= 90 ? 'bg-green-100 text-green-700' : entry.qualityScore >= 70 ? 'bg-blue-100 text-blue-700' : 'bg-amber-100 text-amber-700'}`}>
                          {entry.qualityScore}%
                        </span>
                      </td>
                      <td className="px-6 py-4 text-right">
                        <div className="flex justify-end gap-2">
                          <button
                            onClick={() => startEditing(entry)}
                            className="p-2 text-slate-400 hover:text-blue-600 hover:bg-blue-50 rounded-lg transition-colors"
                            title="Edit"
                          >
                            <Edit2 className="w-4 h-4" />
                          </button>
                          <button
                            onClick={() => {
                              if (confirm('Are you sure you want to delete this TM entry?')) {
                                deleteMutation.mutate(entry.id);
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
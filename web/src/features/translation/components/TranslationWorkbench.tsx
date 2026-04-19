import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api } from '@/api/client';
import TranslationTable from './TranslationTable';
import DocumentSidebar from '@/features/document/components/DocumentSidebar';
import ContextSidebar from './ContextSidebar';

interface Props {
  projectId: string;
}

export default function TranslationWorkbench({ projectId }: Props) {
  const [selectedDocId, setSelectedDocId] = useState<string | null>(null);
  const [page, setPage] = useState(1);

  // Fetch documents for the sidebar
  const { data: docsData } = useQuery({
    queryKey: ['documents', projectId],
    queryFn: () => api.get<any>(`/projects/${projectId}/documents`),
  });

  return (
    <div className="h-full flex overflow-hidden bg-slate-50">
      {/* Left Sidebar: Documents & Filters */}
      <aside className="w-64 shrink-0 bg-white border-r border-slate-200 flex flex-col overflow-hidden">
        <DocumentSidebar 
          documents={docsData?.items || []} 
          selectedDocId={selectedDocId} 
          onSelectDoc={(id) => { setSelectedDocId(id); setPage(1); }} 
        />
      </aside>

      {/* Main Area: Translation Table */}
      <main className="flex-1 flex flex-col min-w-0 overflow-hidden bg-white">
        <div className="h-12 border-b border-slate-200 px-4 flex items-center shrink-0 bg-slate-50/50">
          <h2 className="text-sm font-medium text-slate-700">
            {selectedDocId ? 'Translation Units' : 'All Units'}
          </h2>
        </div>
        <div className="flex-1 overflow-auto">
          <TranslationTable 
            projectId={projectId} 
            documentId={selectedDocId}
            page={page}
            onPageChange={setPage}
          />
        </div>
      </main>

      {/* Right Sidebar: Context / TM / Glossary */}
      <aside className="w-80 shrink-0 bg-slate-50 border-l border-slate-200 flex flex-col overflow-hidden">
        <ContextSidebar projectId={projectId} />
      </aside>
    </div>
  );
}

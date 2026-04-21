import { createFileRoute, Link } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { FileText, LoaderCircle, Search } from 'lucide-react';
import { useState } from 'react';
import { api } from '@/api/client';
import { useTranslation } from '@/hooks/useTranslation';
import { cn } from '@/lib/utils';

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

export const Route = createFileRoute('/project/$projectId/documents')({
  component: DocumentsRoute,
});

function DocumentsRoute() {
  const { projectId } = Route.useParams();
  const { t } = useTranslation();
  const [search, setSearch] = useState('');

  const { data: docsData, isLoading } = useQuery({
    queryKey: ['documents', projectId],
    queryFn: () => api.get<{ items: DocumentItem[] }>(`/projects/${projectId}/documents`),
  });

  const documents = docsData?.items || [];
  const filteredDocuments = search
    ? documents.filter((doc) => `${doc.title} ${doc.path}`.toLowerCase().includes(search.toLowerCase()))
    : documents;

  return (
    <div className="p-8 max-w-7xl mx-auto w-full">
      <div className="mb-8">
        <h1 className="text-3xl font-bold text-slate-900">{t('documents.title')}</h1>
        <p className="mt-2 text-slate-500">{t('documents.subtitle')}</p>
        <div className="mt-4 flex items-center gap-2 rounded-2xl border border-slate-200 bg-white px-4 py-3 shadow-sm">
          <Search className="h-4 w-4 text-slate-400" />
          <input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={t('history.keyFilter')}
            className="w-full bg-transparent text-sm outline-none"
          />
        </div>
      </div>

      {isLoading ? (
        <div className="flex justify-center items-center py-20 text-slate-500">
          <LoaderCircle className="animate-spin w-6 h-6 mr-2" />
          <span>{t('documents.loading')}</span>
        </div>
      ) : filteredDocuments.length === 0 ? (
        <div className="text-center py-20 border border-dashed border-slate-300 rounded-2xl bg-slate-50">
          <p className="text-slate-500">{t('documents.empty')}</p>
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
          {filteredDocuments.map((doc) => (
            <Link
              key={doc.id}
              to="/project/$projectId/workbench"
              params={{ projectId }}
              search={{ documentId: doc.id }}
              className={cn(
                "group relative flex flex-col rounded-2xl border border-slate-200 bg-white p-6 shadow-sm transition-all",
                "hover:border-blue-500 hover:shadow-md hover:-translate-y-1"
              )}
            >
              <div className="flex items-start justify-between">
                <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-blue-50 text-blue-600 group-hover:bg-blue-600 group-hover:text-white transition-colors">
                  <FileText className="h-5 w-5" />
                </div>
              </div>
              <div className="mt-4 flex-1">
                <h3 className="font-semibold text-slate-900 line-clamp-1" title={doc.title || doc.path}>
                  {doc.title || doc.path.split('/').pop()}
                </h3>
                <p className="mt-1 text-sm text-slate-500 line-clamp-2" title={doc.path}>
                  {doc.path}
                </p>
              </div>
              
              {doc.tags && doc.tags.length > 0 && (
                <div className="mt-4 flex flex-wrap gap-2">
                  {doc.tags.map((tag) => (
                    <span
                      key={tag.code}
                      className="inline-flex items-center rounded-md px-2 py-1 text-xs font-medium"
                      style={{ backgroundColor: `${tag.color}20`, color: tag.color }}
                    >
                      {tag.name}
                    </span>
                  ))}
                </div>
              )}
              
              <div className="mt-6 flex items-center text-sm font-medium text-blue-600 opacity-0 group-hover:opacity-100 transition-opacity">
                {t('documents.selectToTranslate')} &rarr;
              </div>
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}

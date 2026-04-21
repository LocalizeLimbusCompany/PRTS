
import { FileJson, Hash } from 'lucide-react';
import { cn } from '@/lib/utils';

interface Document {
  id: string;
  path: string;
  title: string;
  tags?: { code: string; name: string; color: string }[];
}

interface Props {
  documents: Document[];
  selectedDocId: string | null;
  onSelectDoc: (id: string | null) => void;
}

export default function DocumentSidebar({ documents, selectedDocId, onSelectDoc }: Props) {
  return (
    <div className="flex flex-col h-full">
      <div className="p-2 border-b border-slate-200">
        <h3 className="text-sm font-semibold text-slate-800">Documents</h3>
        <input 
          type="text" 
          placeholder="Filter documents..." 
          className="mt-2 w-full px-2 py-1 text-sm border border-slate-300 rounded focus:outline-none focus:border-blue-500"
        />
      </div>
      
      <div className="flex-1 overflow-y-auto p-2 space-y-1">
        <button
          onClick={() => onSelectDoc(null)}
          className={cn(
            "w-full flex items-center px-2 py-1.5 text-sm rounded text-left transition-colors",
            selectedDocId === null ? "bg-blue-50 text-blue-700 font-medium" : "text-slate-700 hover:bg-slate-100"
          )}
        >
          <Hash className="w-4 h-4 mr-2 opacity-50" />
          <span className="truncate">All Units</span>
        </button>

        {documents.map(doc => (
          <button
            key={doc.id}
            onClick={() => onSelectDoc(doc.id)}
            className={cn(
              "w-full flex flex-col items-start px-2 py-1.5 text-sm rounded text-left transition-colors",
              selectedDocId === doc.id ? "bg-blue-50 text-blue-700 font-medium" : "text-slate-700 hover:bg-slate-100"
            )}
          >
            <div className="flex items-center w-full">
              <FileJson className="w-4 h-4 mr-2 shrink-0 opacity-50" />
              <span className="truncate flex-1">{doc.title || doc.path.split('/').pop()}</span>
            </div>
            {doc.tags && doc.tags.length > 0 && (
              <div className="flex gap-1 mt-1 pl-6 flex-wrap">
                {doc.tags.map(tag => (
                  <span 
                    key={tag.code} 
                    className="text-[10px] px-1 py-0.5 rounded border"
                    style={{ borderColor: tag.color, color: tag.color, backgroundColor: `${tag.color}10` }}
                  >
                    {tag.name}
                  </span>
                ))}
              </div>
            )}
          </button>
        ))}
      </div>
    </div>
  );
}

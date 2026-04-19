
import { BookA, MessageSquare } from 'lucide-react';

interface Props {
  projectId: string;
}

export default function ContextSidebar({}: Props) {
  return (
    <div className="flex flex-col h-full">
      <div className="p-3 border-b border-slate-200 bg-white shrink-0">
        <h3 className="text-sm font-semibold text-slate-800 flex items-center">
          <BookA className="w-4 h-4 mr-2" />
          Glossary & Context
        </h3>
      </div>
      <div className="flex-1 overflow-y-auto p-4">
        <div className="text-sm text-slate-500 text-center mt-10">
          <MessageSquare className="w-8 h-8 mx-auto mb-2 opacity-20" />
          <p>Select a translation unit to view related terms and translation memory.</p>
        </div>
      </div>
    </div>
  );
}

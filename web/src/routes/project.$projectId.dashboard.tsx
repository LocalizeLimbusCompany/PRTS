import { createFileRoute } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { api } from '@/api/client';
import { FileText, Type, CheckCircle, Globe2, BarChart3, Activity } from 'lucide-react';

export const Route = createFileRoute('/project/$projectId/dashboard')({
  component: Dashboard,
});

function Dashboard() {
  const { projectId } = Route.useParams();
  
  const { data: project, isLoading } = useQuery({
    queryKey: ['project', projectId],
    queryFn: () => api.get<any>(`/projects/${projectId}`),
  });

  if (isLoading) {
    return (
      <div className="p-8 h-full bg-slate-50 flex items-center justify-center">
        <div className="text-slate-400">Loading project data...</div>
      </div>
    );
  }

  const totalUnits = Number(project?.translationUnitCount ?? 0);
  const statusCounts = {
    untranslated: Number(project?.statusCounts?.untranslated ?? 0),
    translated: Number(project?.statusCounts?.translated ?? 0),
    reviewed: Number(project?.statusCounts?.reviewed ?? 0),
    approved: Number(project?.statusCounts?.approved ?? 0),
  };
  
  const translatedPct = totalUnits > 0 ? (statusCounts.translated / totalUnits) * 100 : 0;
  const reviewedPct = totalUnits > 0 ? (statusCounts.reviewed / totalUnits) * 100 : 0;
  const approvedPct = totalUnits > 0 ? (statusCounts.approved / totalUnits) * 100 : 0;

  return (
    <div className="p-8 h-full overflow-y-auto bg-slate-50">
      <div className="max-w-6xl mx-auto space-y-8">
        <div>
          <h1 className="text-3xl font-bold text-slate-900">{project?.name || 'Project Dashboard'}</h1>
          <p className="mt-2 text-slate-500">{project?.description || 'Overview of translation progress and activity.'}</p>
        </div>

        {/* Top Stats Cards */}
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6">
          <StatCard 
            icon={<FileText className="w-6 h-6 text-blue-600" />} 
            label="Documents" 
            value={project?.documentCount || 0} 
            bg="bg-blue-50"
          />
          <StatCard 
            icon={<Type className="w-6 h-6 text-indigo-600" />} 
            label="Translation Units" 
            value={totalUnits} 
            bg="bg-indigo-50"
          />
          <StatCard 
            icon={<Globe2 className="w-6 h-6 text-emerald-600" />} 
            label="Target Language" 
            value={project?.targetLanguage?.toUpperCase() || '-'} 
            bg="bg-emerald-50"
            isString
          />
          <StatCard 
            icon={<CheckCircle className="w-6 h-6 text-amber-600" />} 
            label="Progress (Audited)" 
            value={`${approvedPct.toFixed(1)}%`} 
            bg="bg-amber-50"
            isString
          />
        </div>

        <div className="grid grid-cols-1 lg:grid-cols-3 gap-8">
          {/* Progress Section */}
          <div className="lg:col-span-2 bg-white rounded-3xl p-8 border border-slate-200 shadow-sm">
            <div className="flex items-center mb-6">
              <BarChart3 className="w-6 h-6 text-slate-400 mr-3" />
              <h2 className="text-xl font-bold text-slate-900">Translation Progress</h2>
            </div>
            
            {/* Main Progress Bar */}
            <div className="h-4 w-full bg-slate-100 rounded-full overflow-hidden flex mb-8">
              <div style={{ width: `${approvedPct}%` }} className="bg-green-500 h-full transition-all duration-500" title={`Audited: ${statusCounts.approved}`}></div>
              <div style={{ width: `${reviewedPct}%` }} className="bg-blue-500 h-full transition-all duration-500" title={`Reviewed: ${statusCounts.reviewed}`}></div>
              <div style={{ width: `${translatedPct}%` }} className="bg-violet-400 h-full transition-all duration-500" title={`Translated: ${statusCounts.translated}`}></div>
            </div>

            {/* Legend & Details */}
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-4">
              <ProgressStat dot="bg-slate-200" label="Untranslated" count={statusCounts.untranslated} />
              <ProgressStat dot="bg-slate-300" label="Translated" count={statusCounts.translated} />
              <ProgressStat dot="bg-blue-500" label="Reviewed" count={statusCounts.reviewed} />
              <ProgressStat dot="bg-green-500" label="Audited" count={statusCounts.approved} />
            </div>
          </div>

          {/* Project Details Sidebar */}
          <div className="bg-white rounded-3xl p-8 border border-slate-200 shadow-sm flex flex-col">
            <div className="flex items-center mb-6">
              <Activity className="w-6 h-6 text-slate-400 mr-3" />
              <h2 className="text-xl font-bold text-slate-900">Project Details</h2>
            </div>
            
            <div className="space-y-6 flex-1">
              <div>
                <div className="text-sm font-semibold text-slate-400 uppercase tracking-wider mb-2">Identifier</div>
                <div className="font-mono text-slate-700 bg-slate-50 px-3 py-2 rounded-lg text-sm border border-slate-100">{project?.slug}</div>
              </div>
              
              <div>
                <div className="text-sm font-semibold text-slate-400 uppercase tracking-wider mb-2">Source Languages</div>
                <div className="flex flex-wrap gap-2">
                  {project?.sourceLanguages?.map((lang: string) => (
                    <span key={lang} className="uppercase font-mono text-xs px-2.5 py-1.5 bg-slate-100 text-slate-600 rounded-md border border-slate-200">
                      {lang}
                    </span>
                  ))}
                </div>
              </div>

              <div>
                <div className="text-sm font-semibold text-slate-400 uppercase tracking-wider mb-2">Visibility</div>
                <span className="capitalize px-3 py-1 bg-slate-100 text-slate-600 rounded-full text-sm font-medium">
                  {project?.visibility || 'Private'}
                </span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function StatCard({ icon, label, value, bg, isString = false }: { icon: React.ReactNode, label: string, value: string | number, bg: string, isString?: boolean }) {
  const displayValue = isString ? value : Number(value ?? 0).toLocaleString();

  return (
    <div className="bg-white rounded-3xl p-6 border border-slate-200 shadow-sm flex items-center gap-5">
      <div className={`w-14 h-14 rounded-2xl ${bg} flex items-center justify-center shrink-0`}>
        {icon}
      </div>
      <div>
        <div className="text-sm font-semibold text-slate-500 mb-1">{label}</div>
        <div className="text-2xl font-bold text-slate-900">{displayValue}</div>
      </div>
    </div>
  );
}

function ProgressStat({ dot, label, count }: { dot: string, label: string, count: number }) {
  return (
    <div className="flex flex-col">
      <div className="flex items-center text-sm font-medium text-slate-600 mb-2">
        <div className={`w-3 h-3 rounded-full ${dot} mr-2`}></div>
        {label}
      </div>
      <div className="text-2xl font-semibold text-slate-900">{Number(count ?? 0).toLocaleString()}</div>
    </div>
  );
}

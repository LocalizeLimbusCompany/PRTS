import { createFileRoute, redirect } from '@tanstack/react-router';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { LoaderCircle, UserPlus, Trash2, Shield } from 'lucide-react';
import { getProjectMembers, upsertProjectMember, deleteProjectMember, getProjectRoles } from '@/api/members';

export const Route = createFileRoute('/project/$projectId/members')({
  beforeLoad: () => {
    if (!localStorage.getItem('auth-storage') || !JSON.parse(localStorage.getItem('auth-storage') || '{}').state?.token) {
      throw redirect({ to: '/login' });
    }
  },
  component: Members,
});

function Members() {
  const { projectId } = Route.useParams();
  const queryClient = useQueryClient();

  const [newUserId, setNewUserId] = useState('');
  const [newRoleCode, setNewRoleCode] = useState('translator');

  const { data: membersData, isLoading: isLoadingMembers } = useQuery({
    queryKey: ['members', projectId],
    queryFn: () => getProjectMembers(projectId),
  });

  const { data: rolesData } = useQuery({
    queryKey: ['roles', projectId],
    queryFn: () => getProjectRoles(projectId),
  });

  const addMemberMutation = useMutation({
    mutationFn: () => upsertProjectMember(projectId, newUserId, newRoleCode),
    onSuccess: () => {
      setNewUserId('');
      queryClient.invalidateQueries({ queryKey: ['members', projectId] });
    },
  });

  const removeMemberMutation = useMutation({
    mutationFn: (userId: string) => deleteProjectMember(projectId, userId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['members', projectId] });
    },
  });

  const updateRoleMutation = useMutation({
    mutationFn: ({ userId, roleCode }: { userId: string; roleCode: string }) => upsertProjectMember(projectId, userId, roleCode),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['members', projectId] });
    },
  });

  const members = membersData?.items || [];
  const roles = rolesData?.items || [
    { code: 'owner', name: 'Owner', description: 'Full access' },
    { code: 'admin', name: 'Admin', description: 'Project administration' },
    { code: 'reviewer', name: 'Reviewer', description: 'Can review and translate' },
    { code: 'translator', name: 'Translator', description: 'Can translate' },
    { code: 'guest', name: 'Guest', description: 'Read only access' },
  ];

  return (
    <div className="p-8 h-full overflow-y-auto bg-slate-50">
      <div className="max-w-5xl mx-auto space-y-8">
        <div>
          <h1 className="text-3xl font-bold text-slate-900">Members & Permissions</h1>
          <p className="mt-2 text-slate-500">Manage who has access to this project and their roles.</p>
        </div>

        {/* Add Member Form */}
        <div className="bg-white p-6 rounded-2xl border border-slate-200 shadow-sm">
          <h2 className="text-lg font-semibold text-slate-900 mb-4">Add Member</h2>
          <div className="flex gap-4 items-end">
            <div className="flex-1">
              <label className="block text-sm font-medium text-slate-700 mb-1">User ID</label>
              <input
                type="text"
                placeholder="UUID of the user"
                value={newUserId}
                onChange={(e) => setNewUserId(e.target.value)}
                className="w-full rounded-lg border border-slate-300 px-4 py-2 outline-none focus:border-blue-500 transition-colors"
              />
            </div>
            <div className="w-48">
              <label className="block text-sm font-medium text-slate-700 mb-1">Role</label>
              <select
                value={newRoleCode}
                onChange={(e) => setNewRoleCode(e.target.value)}
                className="w-full rounded-lg border border-slate-300 px-4 py-2 outline-none focus:border-blue-500 transition-colors bg-white"
              >
                {roles.map(role => (
                  <option key={role.code} value={role.code}>{role.name}</option>
                ))}
              </select>
            </div>
            <button
              onClick={() => addMemberMutation.mutate()}
              disabled={!newUserId || addMemberMutation.isPending}
              className="px-6 py-2 bg-blue-600 text-white rounded-lg font-medium hover:bg-blue-700 disabled:opacity-50 transition-colors flex items-center h-[42px]"
            >
              {addMemberMutation.isPending ? <LoaderCircle className="w-5 h-5 animate-spin" /> : <UserPlus className="w-5 h-5 mr-2" />}
              Add
            </button>
          </div>
          {addMemberMutation.isError && (
            <p className="text-red-500 mt-2 text-sm">Failed to add member. Check if the User ID is correct.</p>
          )}
        </div>

        {/* Member List */}
        <div className="bg-white rounded-2xl border border-slate-200 shadow-sm overflow-hidden">
          <div className="px-6 py-4 border-b border-slate-200 bg-slate-50 flex justify-between items-center">
            <h2 className="text-lg font-semibold text-slate-900 flex items-center">
              <Shield className="w-5 h-5 mr-2 text-blue-600" />
              Project Members
            </h2>
            <span className="text-sm text-slate-500 font-medium bg-white px-3 py-1 rounded-full border border-slate-200">
              {membersData?.total || 0} Members
            </span>
          </div>
          
          {isLoadingMembers ? (
            <div className="flex justify-center items-center py-12 text-slate-500">
              <LoaderCircle className="w-6 h-6 animate-spin mr-2" />
              Loading members...
            </div>
          ) : members.length === 0 ? (
            <div className="py-12 text-center text-slate-500">
              No members found.
            </div>
          ) : (
            <div className="divide-y divide-slate-100">
              {members.map(member => (
                <div key={member.id} className="p-6 flex items-center justify-between hover:bg-slate-50 transition-colors">
                  <div>
                    <div className="font-semibold text-slate-900">{member.user.name}</div>
                    <div className="text-sm text-slate-500 mt-1 font-mono">{member.user.id}</div>
                  </div>
                  <div className="flex items-center gap-4">
                    <select
                      value={member.roleCode}
                      onChange={(e) => updateRoleMutation.mutate({ userId: member.user.id, roleCode: e.target.value })}
                      disabled={updateRoleMutation.isPending && updateRoleMutation.variables?.userId === member.user.id}
                      className="rounded-lg border border-slate-200 px-3 py-1.5 text-sm outline-none focus:border-blue-500 transition-colors bg-white"
                    >
                      {roles.map(role => (
                        <option key={role.code} value={role.code}>{role.name}</option>
                      ))}
                    </select>
                    <button
                      onClick={() => {
                        if (confirm('Are you sure you want to remove this member?')) {
                          removeMemberMutation.mutate(member.user.id);
                        }
                      }}
                      disabled={removeMemberMutation.isPending && removeMemberMutation.variables === member.user.id}
                      className="p-2 text-red-500 hover:bg-red-50 rounded-lg transition-colors"
                      title="Remove Member"
                    >
                      <Trash2 className="w-5 h-5" />
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

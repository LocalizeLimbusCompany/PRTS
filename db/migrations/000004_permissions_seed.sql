INSERT INTO permission_nodes (code, name, description)
VALUES
    ('project.view', '查看项目', '允许查看项目基础信息'),
    ('project.edit', '编辑项目', '允许修改项目信息'),
    ('project.member.manage', '管理成员', '允许管理项目成员'),
    ('project.role.manage', '管理角色', '允许管理项目角色'),
    ('project.permission.manage', '管理权限', '允许管理权限节点和覆写'),
    ('document.view', '查看文档', '允许查看文档'),
    ('document.create', '创建文档', '允许创建文档'),
    ('document.edit', '编辑文档', '允许编辑文档'),
    ('document.tag.manage', '管理标签', '允许管理文档标签'),
    ('translation.view', '查看翻译', '允许查看翻译条目'),
    ('translation.edit', '编辑翻译', '允许编辑翻译条目'),
    ('translation.review', '校对翻译', '允许校对翻译条目'),
    ('translation.approve', '批准翻译', '允许批准翻译条目'),
    ('translation.history.view', '查看历史', '允许查看翻译历史'),
    ('version.view', '查看版本', '允许查看文档版本')
ON CONFLICT (code) DO NOTHING;

INSERT INTO project_roles (id, project_id, code, name, is_system)
VALUES
    ('f1111111-1111-1111-1111-111111111111', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'owner', 'Owner', TRUE),
    ('f2222222-2222-2222-2222-222222222222', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'admin', 'Admin', TRUE),
    ('f3333333-3333-3333-3333-333333333333', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'reviewer', 'Reviewer', TRUE),
    ('f4444444-4444-4444-4444-444444444444', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'translator', 'Translator', TRUE),
    ('f5555555-5555-5555-5555-555555555555', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'guest', 'Guest', TRUE)
ON CONFLICT (project_id, code) DO NOTHING;

INSERT INTO project_members (project_id, user_id, role_code)
VALUES
    ('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', '11111111-1111-1111-1111-111111111111', 'owner'),
    ('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', '22222222-2222-2222-2222-222222222222', 'reviewer')
ON CONFLICT (project_id, user_id) DO NOTHING;

INSERT INTO project_role_permissions (project_role_id, permission_node_id)
SELECT role_map.id, pn.id
FROM (
    VALUES
        ('owner', 'project.view'),
        ('owner', 'project.edit'),
        ('owner', 'project.member.manage'),
        ('owner', 'project.role.manage'),
        ('owner', 'project.permission.manage'),
        ('owner', 'document.view'),
        ('owner', 'document.create'),
        ('owner', 'document.edit'),
        ('owner', 'document.tag.manage'),
        ('owner', 'translation.view'),
        ('owner', 'translation.edit'),
        ('owner', 'translation.review'),
        ('owner', 'translation.approve'),
        ('owner', 'translation.history.view'),
        ('owner', 'version.view'),
        ('admin', 'project.view'),
        ('admin', 'project.edit'),
        ('admin', 'document.view'),
        ('admin', 'document.create'),
        ('admin', 'document.edit'),
        ('admin', 'document.tag.manage'),
        ('admin', 'translation.view'),
        ('admin', 'translation.edit'),
        ('admin', 'translation.review'),
        ('admin', 'translation.approve'),
        ('admin', 'translation.history.view'),
        ('admin', 'version.view'),
        ('reviewer', 'project.view'),
        ('reviewer', 'document.view'),
        ('reviewer', 'translation.view'),
        ('reviewer', 'translation.edit'),
        ('reviewer', 'translation.review'),
        ('reviewer', 'translation.approve'),
        ('reviewer', 'translation.history.view'),
        ('reviewer', 'version.view'),
        ('translator', 'project.view'),
        ('translator', 'document.view'),
        ('translator', 'translation.view'),
        ('translator', 'translation.edit'),
        ('translator', 'translation.history.view'),
        ('guest', 'project.view'),
        ('guest', 'document.view'),
        ('guest', 'translation.view'),
        ('guest', 'translation.history.view'),
        ('guest', 'version.view')
) AS role_permission(role_code, permission_code)
JOIN project_roles role_map
  ON role_map.project_id = 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb'
 AND role_map.code = role_permission.role_code
JOIN permission_nodes pn
  ON pn.code = role_permission.permission_code
ON CONFLICT (project_role_id, permission_node_id) DO NOTHING;

INSERT INTO project_member_permission_overrides (project_id, user_id, permission_node_id, effect)
SELECT 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', '22222222-2222-2222-2222-222222222222', id, 'allow'
FROM permission_nodes
WHERE code = 'document.tag.manage'
ON CONFLICT (project_id, user_id, permission_node_id) DO NOTHING;

INSERT INTO project_member_document_rules (project_id, user_id, permission_scope, match_type, match_value, effect)
VALUES
    ('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', '22222222-2222-2222-2222-222222222222', 'translation.edit', 'path_prefix', 'StoryData/', 'allow'),
    ('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', '22222222-2222-2222-2222-222222222222', 'translation.edit', 'tag', 'event', 'deny')
ON CONFLICT DO NOTHING;

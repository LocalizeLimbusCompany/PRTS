INSERT INTO users (id, email, username, password_hash, display_name, preferred_locale, preferred_source_language)
VALUES
    ('11111111-1111-1111-1111-111111111111', 'admin@example.com', 'admin', '$2a$10$VRt8Z35mWSN.dCJNSj3f8OQ2XE/mSsZ/rVg4oHdtsZu6zfckxNfXa', 'Amiya', 'zh-CN', 'jp'),
    ('22222222-2222-2222-2222-222222222222', 'reviewer@example.com', 'reviewer', '$2a$10$tm/ecu/2Co4MMCbOjpPzQOARuvMGwnmtXHv9QAAPg27Ea5dBrO1a6', 'Kal''tsit', 'zh-CN', 'en')
ON CONFLICT (email) DO UPDATE SET
    password_hash = EXCLUDED.password_hash,
    display_name = EXCLUDED.display_name,
    preferred_locale = EXCLUDED.preferred_locale,
    preferred_source_language = EXCLUDED.preferred_source_language;

INSERT INTO organizations (id, slug, name, description, visibility, created_by)
VALUES
    ('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'rhodes-island', 'Rhodes Island', '公开翻译协作组织', 'public', '11111111-1111-1111-1111-111111111111')
ON CONFLICT (slug) DO NOTHING;

INSERT INTO projects (id, organization_id, slug, name, description, target_language, visibility, guest_policy, created_by)
VALUES
    ('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'arknights-cn', 'Arknights-CN', '明日方舟中文翻译项目', 'cn', 'public', 'read', '11111111-1111-1111-1111-111111111111')
ON CONFLICT (organization_id, slug) DO NOTHING;

INSERT INTO project_source_languages (project_id, language_code, sort_order)
VALUES
    ('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'en', 1),
    ('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'jp', 2),
    ('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'kr', 3)
ON CONFLICT (project_id, language_code) DO NOTHING;

INSERT INTO document_tags (id, project_id, code, name, color, is_visible)
VALUES
    ('cccccccc-cccc-cccc-cccc-ccccccccccc1', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'main', '主线', '#4C88FF', TRUE),
    ('cccccccc-cccc-cccc-cccc-ccccccccccc2', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'event', '活动', '#58A55C', TRUE)
ON CONFLICT (project_id, code) DO NOTHING;

INSERT INTO documents (id, project_id, path, title, current_version_no, updated_by)
VALUES
    ('dddddddd-dddd-dddd-dddd-ddddddddddd1', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'StoryData/S001.json', 'S001', 2, '11111111-1111-1111-1111-111111111111'),
    ('dddddddd-dddd-dddd-dddd-ddddddddddd2', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'StoryData/S002.json', 'S002', 1, '22222222-2222-2222-2222-222222222222')
ON CONFLICT (project_id, path) DO NOTHING;

INSERT INTO document_tag_bindings (document_id, tag_id)
VALUES
    ('dddddddd-dddd-dddd-dddd-ddddddddddd1', 'cccccccc-cccc-cccc-cccc-ccccccccccc1'),
    ('dddddddd-dddd-dddd-dddd-ddddddddddd2', 'cccccccc-cccc-cccc-cccc-ccccccccccc2')
ON CONFLICT (document_id, tag_id) DO NOTHING;

INSERT INTO document_versions (document_id, version_no, source_snapshot_hash, created_by)
VALUES
    ('dddddddd-dddd-dddd-dddd-ddddddddddd1', 1, 'seed-hash-s001-v1', '11111111-1111-1111-1111-111111111111'),
    ('dddddddd-dddd-dddd-dddd-ddddddddddd1', 2, 'seed-hash-s001-v2', '22222222-2222-2222-2222-222222222222'),
    ('dddddddd-dddd-dddd-dddd-ddddddddddd2', 1, 'seed-hash-s002-v1', '11111111-1111-1111-1111-111111111111')
ON CONFLICT (document_id, version_no) DO NOTHING;

INSERT INTO translation_units (id, project_id, document_id, key, target_language, target_text, status, comment, version_no, updated_by)
VALUES
    ('eeeeeeee-eeee-eeee-eeee-eeeeeeeeeee1', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'dddddddd-dddd-dddd-dddd-ddddddddddd1', 'story.line.001', 'cn', '博士，醒醒。', 'translated', '按当前语境微调', 3, '11111111-1111-1111-1111-111111111111'),
    ('eeeeeeee-eeee-eeee-eeee-eeeeeeeeeee2', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'dddddddd-dddd-dddd-dddd-ddddddddddd1', 'story.line.002', 'cn', '阿米娅正在等你。', 'reviewed', '语气已统一', 2, '22222222-2222-2222-2222-222222222222'),
    ('eeeeeeee-eeee-eeee-eeee-eeeeeeeeeee3', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'dddddddd-dddd-dddd-dddd-ddddddddddd2', 'story.line.003', 'cn', '', 'untranslated', '', 1, '11111111-1111-1111-1111-111111111111')
ON CONFLICT (document_id, key) DO NOTHING;

INSERT INTO translation_unit_sources (translation_unit_id, language_code, text)
VALUES
    ('eeeeeeee-eeee-eeee-eeee-eeeeeeeeeee1', 'en', 'Doctor, wake up.'),
    ('eeeeeeee-eeee-eeee-eeee-eeeeeeeeeee1', 'jp', 'ドクター、起きてください。'),
    ('eeeeeeee-eeee-eeee-eeee-eeeeeeeeeee1', 'kr', '박사님, 일어나세요.'),
    ('eeeeeeee-eeee-eeee-eeee-eeeeeeeeeee2', 'en', 'Amiya is waiting for you.'),
    ('eeeeeeee-eeee-eeee-eeee-eeeeeeeeeee2', 'jp', 'アーミヤが待っています。'),
    ('eeeeeeee-eeee-eeee-eeee-eeeeeeeeeee2', 'kr', '아미야가 기다리고 있어요.'),
    ('eeeeeeee-eeee-eeee-eeee-eeeeeeeeeee3', 'en', 'The operation is about to begin.'),
    ('eeeeeeee-eeee-eeee-eeee-eeeeeeeeeee3', 'jp', '作戦がまもなく始まります。'),
    ('eeeeeeee-eeee-eeee-eeee-eeeeeeeeeee3', 'kr', '작전이 곧 시작됩니다.')
ON CONFLICT (translation_unit_id, language_code) DO NOTHING;

INSERT INTO translation_revisions (translation_unit_id, revision_no, before_target_text, after_target_text, before_status, after_status, change_note, changed_by)
VALUES
    ('eeeeeeee-eeee-eeee-eeee-eeeeeeeeeee1', 1, '', '医生，醒来。', 'untranslated', 'translated', '初版翻译', '11111111-1111-1111-1111-111111111111'),
    ('eeeeeeee-eeee-eeee-eeee-eeeeeeeeeee1', 2, '医生，醒来。', '博士，醒醒。', 'translated', 'translated', '统一术语', '22222222-2222-2222-2222-222222222222'),
    ('eeeeeeee-eeee-eeee-eeee-eeeeeeeeeee2', 1, '', '阿米娅正在等你。', 'untranslated', 'reviewed', '校对通过', '22222222-2222-2222-2222-222222222222')
ON CONFLICT (translation_unit_id, revision_no) DO NOTHING;

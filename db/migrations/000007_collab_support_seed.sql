INSERT INTO glossary_terms (project_id, source_term, target_term, source_language, target_language, note, created_by)
VALUES
    ('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'Doctor', '博士', 'en', 'cn', '主角称呼统一', '11111111-1111-1111-1111-111111111111'),
    ('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'Rhodes Island', '罗德岛', 'en', 'cn', '组织名统一', '11111111-1111-1111-1111-111111111111')
ON CONFLICT DO NOTHING;

INSERT INTO translation_memory_entries (project_id, source_language, target_language, source_text, target_text, quality_score, created_from_unit_id)
VALUES
    ('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'en', 'cn', 'Doctor, wake up.', '博士，醒醒。', 90, 'eeeeeeee-eeee-eeee-eeee-eeeeeeeeeee1'),
    ('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'jp', 'cn', 'アーミヤが待っています。', '阿米娅正在等你。', 88, 'eeeeeeee-eeee-eeee-eeee-eeeeeeeeeee2')
ON CONFLICT DO NOTHING;

INSERT INTO import_jobs (id, project_id, document_path, status, uploaded_by, started_at, finished_at)
VALUES
    ('12121212-1212-1212-1212-121212121212', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'StoryData/S001.json', 'finished', '11111111-1111-1111-1111-111111111111', NOW() - INTERVAL '2 days', NOW() - INTERVAL '2 days')
ON CONFLICT (id) DO NOTHING;

INSERT INTO export_jobs (id, project_id, status, file_path, file_size, expires_at, requested_by, started_at, finished_at)
VALUES
    ('13131313-1313-1313-1313-131313131313', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'finished', '/exports/arknights-cn-export-demo.zip', 20480, NOW() + INTERVAL '1 day', '11111111-1111-1111-1111-111111111111', NOW() - INTERVAL '1 hour', NOW() - INTERVAL '1 hour')
ON CONFLICT (id) DO NOTHING;

INSERT INTO audit_logs (organization_id, project_id, user_id, action, resource_type, resource_id, detail_json)
VALUES
    ('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', '11111111-1111-1111-1111-111111111111', 'project.create', 'project', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', '{"name":"Arknights-CN"}'::jsonb),
    ('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', '22222222-2222-2222-2222-222222222222', 'translation.review', 'translation_unit', 'eeeeeeee-eeee-eeee-eeee-eeeeeeeeeee2', '{"status":"reviewed"}'::jsonb)
ON CONFLICT DO NOTHING;

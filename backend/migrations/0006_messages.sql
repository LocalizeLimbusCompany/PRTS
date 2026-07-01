-- 0006_messages.sql —— 持久私信（类微信）。
--
-- 会话即「一对用户之间的消息集合」，不单独建 conversations 表（见 Spec D §2）。
-- 收发双方须共享 ≥1 项目的校验在应用层（prts-api）完成，DB 仅存储与索引。
CREATE TABLE messages (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    -- 发送者；用户注销时其消息一并级联删除。
    sender_id    BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- 收件人；同上级联。
    recipient_id BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- 消息正文（应用层限制 ≤ 2000 字）。
    content      TEXT NOT NULL,
    -- 收件人读取时间；NULL = 未读。
    read_at      TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 会话查询（键集分页，按 id 降序）：正、反两个方向各建复合索引，
-- 覆盖 list_conversation 的 (sender,recipient) OR (recipient,sender) 双向条件。
CREATE INDEX messages_pair_idx     ON messages (sender_id, recipient_id, id DESC);
CREATE INDEX messages_pair_rev_idx ON messages (recipient_id, sender_id, id DESC);
-- 收件人未读数（unread_count / 会话未读徽标）：部分索引只覆盖未读行，代价小。
CREATE INDEX messages_unread_idx   ON messages (recipient_id) WHERE read_at IS NULL;

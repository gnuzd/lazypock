-- 0019_mail_templates.sql
--
-- Mail templates that can be edited while the server runs.
--
-- Until now a template was a file in the template directory, which needs a deploy to
-- change, or the built-in text, which needs a release. This is the third layer: a
-- body saved through the API, which wins over both. A deployment can therefore fix a
-- typo in a password-reset email without rebuilding anything.
--
-- Nothing is seeded. Writing a copy of every built-in on first boot would shadow
-- later improvements to the built-in forever, and it would make "reset" mean
-- "restore the copy taken at boot" rather than "fall back to the file or the
-- built-in". A row exists only once an operator saves something.

create table if not exists _mail_templates (
    name       text        not null,
    part       text        not null,
    body       text        not null,
    created    timestamptz not null default now(),
    updated    timestamptz not null default now(),
    primary key (name, part),
    constraint _mail_templates_part_check
        check (part in ('subject', 'html', 'text')),
    constraint _mail_templates_name_check
        check (length(btrim(name)) between 1 and 64),
    constraint _mail_templates_body_check
        check (length(body) between 1 and 65536)
);

comment on table _mail_templates is
    'Mail template bodies saved through the API. A body here wins over a file in the template directory, which wins over the built-in text.';

-- `0003_rls.sql` sets default privileges that hand every table created in this schema
-- to the restricted roles, so this table arrives with select, insert, update and
-- delete granted to `lazypock_anon` and `lazypock_authenticated`. It must not be
-- reachable by a request at all: a template is rendered with the variables of an auth
-- email — a verification or password-reset token, the recipient's address — so
-- whoever can write a template can mail themselves somebody else's token. The API
-- writes this table as the schema owner, which is not one of these roles.
revoke all on _mail_templates from lazypock_anon, lazypock_authenticated;

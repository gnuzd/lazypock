-- `token_key` is a column, not a field.
--
-- 0006 recorded `token_key` as a field alongside the other auth system columns. It
-- is not one: the server generates it on account creation and rotates it on a
-- password change, nothing ever sets it from input, and a client reads a collection's
-- **definition** — so recording it there only put a server-managed credential-shaped
-- value in front of every client, including the Studio's record form (which rendered
-- it as an ordinary text input, because it does not honour `hidden`).
--
-- The column stays: the auth layer writes it and reads it back per account. It is
-- simply no longer advertised as part of the collection. `password_hash` stays a
-- field, because the Studio needs it to render its password input.
--
-- Idempotent: a database that never recorded the row loses nothing.

delete from _fields f
using _collections c
where f.collection_id = c.id
  and c.type = 'auth'
  and f.name = 'token_key';

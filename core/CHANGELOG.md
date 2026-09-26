# Changelog

## [0.14.0](https://github.com/gnuzd/lazypock/compare/v0.13.0...v0.14.0) (2026-09-26)


### Features

* **api:** accept backup archives over HTTP with a scoped body limit ([b4f5475](https://github.com/gnuzd/lazypock/commit/b4f5475560ccbfe08ef386577c8dec084f4b760c))
* **backup:** include uploaded files in the archive, and preview archives ([78d34f7](https://github.com/gnuzd/lazypock/commit/78d34f7a419a0a6bee45ff95da19bea1aa3b47ea))
* **backup:** stream large databases as an NDJSON archive ([b048682](https://github.com/gnuzd/lazypock/commit/b048682403ef814ec0aeda5c351243c9c15a2417))
* **backup:** stream large databases as an NDJSON archive ([1b3dc52](https://github.com/gnuzd/lazypock/commit/1b3dc528d71d1c6da7c2587780fd4c7863757244))
* **studio:** archive backup/restore with progress and a large-import gate ([8da4b71](https://github.com/gnuzd/lazypock/commit/8da4b715dfe9c183cb4687d764a550522da3c2ad))

## [0.13.0](https://github.com/gnuzd/lazypock/compare/v0.12.1...v0.13.0) (2026-09-24)


### Features

* **import:** atomic restore + one-click rollback ([cbd18d7](https://github.com/gnuzd/lazypock/commit/cbd18d776fbaf09979e216d0a23bebbad29e4d18))
* **import:** atomic restore + one-click rollback ([3a1d3b6](https://github.com/gnuzd/lazypock/commit/3a1d3b6fe356f48769ded6081f12557351c9cd80))
* **security:** restrict + audit + re-auth import/export/rollback ([82068e9](https://github.com/gnuzd/lazypock/commit/82068e97d3fcae1d8e10d87c4a7d4c19a7e9893d))
* **security:** superuser-only import/export — guard tests, audit, re-auth ([1a76b92](https://github.com/gnuzd/lazypock/commit/1a76b923aebb6918b191737d6d6d2db11572a1c4))
* **studio:** add a "Copy AI prompt" button to Import and Backups ([e54a22a](https://github.com/gnuzd/lazypock/commit/e54a22a5aaeefb07cc45e69fe4124813b9bf7057))
* **studio:** confirm password for import / restore / rollback ([eb2b997](https://github.com/gnuzd/lazypock/commit/eb2b997337094d83bc141b20b969d42ff2d73af8))
* **studio:** Copy AI prompt for import/restore ([e4bfa8e](https://github.com/gnuzd/lazypock/commit/e4bfa8e50308bd864a5c4c839d6280896935f08c))


### Bug Fixes

* collection deletion (unmanaged 400 + SDK fire-and-forget race) ([79dd06c](https://github.com/gnuzd/lazypock/commit/79dd06ce550c58888d491caba8ab27733cf6e47c))
* **core:** allow deleting unmanaged collections (they were undeletable) ([9ef031f](https://github.com/gnuzd/lazypock/commit/9ef031f23a3c4abc3c0d1d06fccd1c4eefc7e5ef))
* **core:** make the auth-collection email field unique by default ([9af5c76](https://github.com/gnuzd/lazypock/commit/9af5c76a877fd741b28f08d0cd4fa5fa358e5d59))
* **core:** make the auth-collection email field unique by default ([6137640](https://github.com/gnuzd/lazypock/commit/61376408105b4f9112b1e3fc826cde8f4d4eb125))
* **import:** report the real error instead of a 25P02 cascade ([b3a70b1](https://github.com/gnuzd/lazypock/commit/b3a70b1088857186bcc04c199c628e6d24703fdd))
* **studio:** animate only .loading-spinner, not the .loading state hook ([5148846](https://github.com/gnuzd/lazypock/commit/5148846e55501ece01889d8bc89016cd59f7cb7c))
* **studio:** await collection delete instead of racing the refresh ([b33ddbd](https://github.com/gnuzd/lazypock/commit/b33ddbd28e850d60ec4d447a0bb29f1741c51263))
* **studio:** clear deleted collection's table + modal for rollback ([4eb7097](https://github.com/gnuzd/lazypock/commit/4eb70974c95be436981251870a7e33d944e9cc50))
* **studio:** clear the deleted collection's table + modal for rollback ([1280008](https://github.com/gnuzd/lazypock/commit/1280008ea773da2a4bba9dfdf6d2867864293787))
* **studio:** give icon buttons a real extra-small size ([766cf5e](https://github.com/gnuzd/lazypock/commit/766cf5e4f5c87084e2def20a8000a1cec21fc434))
* **studio:** keep the button label from spinning with the loader ([8179036](https://github.com/gnuzd/lazypock/commit/817903650c1259658bb60033ef69e3ae8b1f9f1e))
* **studio:** unblock index creation and stabilize the record table ([7afad21](https://github.com/gnuzd/lazypock/commit/7afad217903cd89ea0a337cc60a9927b1797d100))
* **studio:** unblock index creation and stabilize the record table ([9862aa3](https://github.com/gnuzd/lazypock/commit/9862aa3a087cf9ae6220590bc3eaf3753a3e87ff))

## [0.12.1](https://github.com/gnuzd/lazypock/compare/v0.12.0...v0.12.1) (2026-09-24)


### Bug Fixes

* **ci:** parse the coverage table in both Elixir 1.17 and 1.18+ formats ([2952119](https://github.com/gnuzd/lazypock/commit/2952119ba2a179cde69e1eaa5898a677bc8cf2ff))
* **rules:** fail closed on rules the enforcer cannot evaluate ([5edd370](https://github.com/gnuzd/lazypock/commit/5edd370b375b1678ab7a6685c2221adbc568650c))
* **rules:** fail closed on rules the enforcer cannot evaluate ([87e1ef2](https://github.com/gnuzd/lazypock/commit/87e1ef2e7f57bf58d572fe670fb4cb09724fb148))

## [0.12.0](https://github.com/gnuzd/lazypock/compare/v0.11.0...v0.12.0) (2026-09-24)


### Features

* **collections:** preview endpoint for the view builder ([15e60c8](https://github.com/gnuzd/lazypock/commit/15e60c8a4661dd8a38f9af3a58c026b0579a345a))
* **schema:** no-code view builder query generator ([93e7f66](https://github.com/gnuzd/lazypock/commit/93e7f66f13d7f02fdf1ad27a6c26dd5ede5dbbac))
* **schema:** no-code view builder query generator ([7753252](https://github.com/gnuzd/lazypock/commit/7753252e874519fc914ac22693176a265b5f6f3d))
* **schema:** wire the view builder into DDL and the collections API ([bbfa76c](https://github.com/gnuzd/lazypock/commit/bbfa76c5da4f7a8bca49191d37073eae44e925ba))
* **studio:** no-code view builder UI ([251dbaa](https://github.com/gnuzd/lazypock/commit/251dbaae56ceca4702251ab869f77298280e8fed))


### Bug Fixes

* **collections:** allow updating a view collection from the Studio ([a0029d4](https://github.com/gnuzd/lazypock/commit/a0029d4fb3d8ee247ac67bd2748b185f9e5c69fc))
* **collections:** allow updating a view collection from the Studio ([e6f931b](https://github.com/gnuzd/lazypock/commit/e6f931b4b4020d0a07b0972151aefdef573f013c))
* **studio:** show table column names verbatim ([d818cb7](https://github.com/gnuzd/lazypock/commit/d818cb793fc0e49388b0db51520c964a6a34c8f7))
* **studio:** show table column names verbatim ([9c95db0](https://github.com/gnuzd/lazypock/commit/9c95db0df1994ce099552a383ea0fbbf607e5e61))
* **studio:** view builder feedback + view realtime test coverage ([125a3f5](https://github.com/gnuzd/lazypock/commit/125a3f57e7e584cad7ecd08b3c63327f10d02e33))
* **studio:** view builder feedback + view realtime test coverage ([4b31e31](https://github.com/gnuzd/lazypock/commit/4b31e315cdd52f9e6e6471561dcc0fb6426ad30e))

## [0.11.0](https://github.com/gnuzd/lazypock/compare/v0.10.2...v0.11.0) (2026-09-22)


### Features

* **filters:** PocketBase ? operators for array fields ([3770177](https://github.com/gnuzd/lazypock/commit/37701779810dc16e491f466f591043b9404b8c91))
* **filters:** PocketBase ? operators for array fields ([0117316](https://github.com/gnuzd/lazypock/commit/0117316df4c5d81a21ad409e083ea7d1e6f33cc3))
* **studio:** add filter, sort and always-on record count to record tables ([0b6eb16](https://github.com/gnuzd/lazypock/commit/0b6eb16c4272c3360ee6b4cbf58efd8e6c77b535))


### Bug Fixes

* data import/export support camelCase and pb latest version ([8e570f5](https://github.com/gnuzd/lazypock/commit/8e570f5d4dbaf3e70437bc919e555b15192f05c5))
* **schema:** keep field names verbatim as DB columns + system timestamps ([5f3eac2](https://github.com/gnuzd/lazypock/commit/5f3eac2b39c337f2c64304e3dd7b64117ebef2a9))
* **schema:** keep field names verbatim as DB columns + system timestamps ([13c8d6d](https://github.com/gnuzd/lazypock/commit/13c8d6d472fe121ebd7e9736d09c407bf8278715))

## [0.10.2](https://github.com/gnuzd/lazypock/compare/v0.10.1...v0.10.2) (2026-09-20)


### Bug Fixes

* reject legacy options ([7152abd](https://github.com/gnuzd/lazypock/commit/7152abda40754c11f918cbe6d904f95a99318d47))
* test validate_field ([0333017](https://github.com/gnuzd/lazypock/commit/03330177b28757dc12aeb1bceec688d78d1c6fce))
* update backup/restore pb type ([04c1c5d](https://github.com/gnuzd/lazypock/commit/04c1c5dfc8c8a5a30d60b08f9b401386e13836e1))

## [0.10.1](https://github.com/gnuzd/lazypock/compare/v0.10.0...v0.10.1) (2026-09-19)


### Bug Fixes

* missing form fields settings import ([b622d73](https://github.com/gnuzd/lazypock/commit/b622d737a4d5eac42eafe6ffe1901f3a2752b9bc))
* missing form fields settings import ([d059af5](https://github.com/gnuzd/lazypock/commit/d059af5721c97c7f8420eb6e73d938a99d16531b))

## [0.10.0](https://github.com/gnuzd/lazypock/compare/v0.9.0...v0.10.0) (2026-09-19)


### Features

* update super form ([63d8611](https://github.com/gnuzd/lazypock/commit/63d86116777e0a47e74f25e8b20c13187159116e))


### Bug Fixes

* polish ui ([0656bc5](https://github.com/gnuzd/lazypock/commit/0656bc58f19167e293d49baeea0b3987683debd2))

## [0.9.0](https://github.com/gnuzd/lazypock/compare/v0.8.1...v0.9.0) (2026-09-01)


### Features

* **schema:** support autodate field type with write-path maintenance ([28cbb17](https://github.com/gnuzd/lazypock/commit/28cbb17e0a1c5486714748c2c9d0fcbf81f3e021))
* **schema:** support autodate field type with write-path maintenance ([7b3e872](https://github.com/gnuzd/lazypock/commit/7b3e8720c9563521451368660e2d95be19fb38d4))


### Bug Fixes

* **migrations:** fail fast on unresolvable relation targets ([0fb9a85](https://github.com/gnuzd/lazypock/commit/0fb9a8571c2e50d9c72c7acc6f83188b06feefd0))
* **migrations:** fail fast on unresolvable relation targets ([9bd81d4](https://github.com/gnuzd/lazypock/commit/9bd81d43d4fe24dbfb0ddafa6cedca34d91d91bf))

## [0.8.1](https://github.com/gnuzd/lazypock/compare/v0.8.0...v0.8.1) (2026-08-31)


### Bug Fixes

* **core,studio:** relation dropdowns for migrated tables, clean test DB logs, docs ([c80fd11](https://github.com/gnuzd/lazypock/commit/c80fd11bc41589d67d4344c5a2a184ef70a59983))

## [0.8.0](https://github.com/gnuzd/lazypock/compare/v0.7.1...v0.8.0) (2026-08-30)


### Features

* **core:** view collections with realtime (PocketBase parity) ([c5f2e92](https://github.com/gnuzd/lazypock/commit/c5f2e9273fdb414a512497018514f306c2a1f832))
* **studio:** collections overview modal (PocketBase parity) ([7d8570c](https://github.com/gnuzd/lazypock/commit/7d8570ccfff95658805b798b17a44b8cfdc724e0))
* **studio:** rebuild ERD on Svelte Flow (zoom/pan, relation edges) ([ac09a0d](https://github.com/gnuzd/lazypock/commit/ac09a0d832d992a2a4afbd6c0b2ca97e145122c9))
* **studio:** view collection UX ([fed70da](https://github.com/gnuzd/lazypock/commit/fed70da60c20dcc1b7ca9ec84bb8da4e5593ad0c))


### Bug Fixes

* **realtime:** own the view-diff ETS snapshot in the Registry ([927cff5](https://github.com/gnuzd/lazypock/commit/927cff51c1cf663f615d4a2ba792035b219353a0))

## [0.7.1](https://github.com/gnuzd/lazypock/compare/v0.7.0...v0.7.1) (2026-08-30)


### Bug Fixes

* **studio:** always request all fields for record reads ([607ad61](https://github.com/gnuzd/lazypock/commit/607ad61569ad0f69ea0751d29bebc58dc83d3f7b))
* **studio:** always request all fields for record reads (stale schema projection drops new fields) ([530bdab](https://github.com/gnuzd/lazypock/commit/530bdaba5d6cee159671703779e48aeb6e4555a0))

## [0.7.0](https://github.com/gnuzd/lazypock/compare/v0.6.2...v0.7.0) (2026-08-29)


### Features

* **docs:** add SvelteKit documentation site for lazypock-ts ([151b12a](https://github.com/gnuzd/lazypock/commit/151b12a6ddfd0ff8b9daa68c0d9b823d19e38321))

## [0.6.2](https://github.com/gnuzd/lazypock/compare/v0.6.1...v0.6.2) (2026-08-28)


### Bug Fixes

* **enforcer:** deny instead of crashing on malformed record id in rule eval ([c1f4ed0](https://github.com/gnuzd/lazypock/commit/c1f4ed023215f5fecfcaf878bb8f04e6595114f5))
* **enforcer:** deny instead of crashing on malformed record id in rule eval ([25581d1](https://github.com/gnuzd/lazypock/commit/25581d1a27c169c2b92715215446558f352ad4ae))

## [0.6.1](https://github.com/gnuzd/lazypock/compare/v0.6.0...v0.6.1) (2026-08-25)


### Bug Fixes

* **cors:** echo requested headers in preflight so X-Connection-Id is allowed ([4bbe37f](https://github.com/gnuzd/lazypock/commit/4bbe37fa5b83ec44751983cca0ac89680db1e179))

## [0.6.0](https://github.com/gnuzd/lazypock/compare/v0.5.1...v0.6.0) (2026-08-25)


### Features

* **migrations:** auto-register raw tables as collections + realtime id support ([63defa8](https://github.com/gnuzd/lazypock/commit/63defa80a64b8a31a2b934d9d9d29ee511e29f0f))


### Bug Fixes

* **migrations:** auto-register raw tables as collections; realtime origin-exclusion for SDK clients ([d80d134](https://github.com/gnuzd/lazypock/commit/d80d1340df9a46930362f7fe21992c3dbbb6d542))

## [0.5.1](https://github.com/gnuzd/lazypock/compare/v0.5.0...v0.5.1) (2026-08-24)


### Bug Fixes

* register collections from user migrations + add backup restore ([fdeae88](https://github.com/gnuzd/lazypock/commit/fdeae88de10e416731d7b141ee7efd924bd9fea1))

## [0.5.0](https://github.com/gnuzd/lazypock/compare/v0.4.5...v0.5.0) (2026-08-23)


### Features

* **realtime:** exclude origin connection, custom channels, admin channel fix ([a6bf6e4](https://github.com/gnuzd/lazypock/commit/a6bf6e41149372d0d55534a30af2978a5801f5e8))
* **realtime:** exclude origin connection, custom channels, admin channel fix ([68b0f30](https://github.com/gnuzd/lazypock/commit/68b0f30a6b5f9d8bc070614437caa075317985be))

## [0.4.5](https://github.com/gnuzd/lazypock/compare/v0.4.4...v0.4.5) (2026-08-22)


### Bug Fixes

* **core:** guard hide-password migration against missing boot-time tables ([e8ce84d](https://github.com/gnuzd/lazypock/commit/e8ce84dbbf45ea9087e21646d726825cbb8ead12))
* **core:** guard hide-password migration against missing boot-time tables ([39206cc](https://github.com/gnuzd/lazypock/commit/39206cc344a9248ff2f6f29a34219e3528b432eb))

## [0.4.4](https://github.com/gnuzd/lazypock/compare/v0.4.3...v0.4.4) (2026-08-22)


### Bug Fixes

* **collections:** users is a normal auth collection, not system ([094866c](https://github.com/gnuzd/lazypock/commit/094866cc0235b41a10634e4c0b18800d696667f8))
* **collections:** users is a normal auth collection, not system ([fca447f](https://github.com/gnuzd/lazypock/commit/fca447fddb46dc391646665b970ba70e9ac08db2))
* **users:** accept `password` alias, hide password fields ([5eb30f6](https://github.com/gnuzd/lazypock/commit/5eb30f681b44579518784e162e26ca62d5f7a3c2))
* **users:** accept password alias, hide password fields ([2392c11](https://github.com/gnuzd/lazypock/commit/2392c1127943f5f4aa6961b066cae0f39df6306d))

## [0.4.3](https://github.com/gnuzd/lazypock/compare/v0.4.2...v0.4.3) (2026-08-22)


### Bug Fixes

* **import:** toast results, drop posts seed, self-heal missing users collection ([fc10943](https://github.com/gnuzd/lazypock/commit/fc109431b72aeb7f4176866296075264ad1ba995))
* **import:** toast results, drop posts seed, self-heal missing users collection ([d805cb7](https://github.com/gnuzd/lazypock/commit/d805cb7b71b9dbebe92d62fea5ee4cd06c727307))

## [0.4.2](https://github.com/gnuzd/lazypock/compare/v0.4.1...v0.4.2) (2026-08-22)


### Bug Fixes

* **import:** keep field names verbatim — no snake/camel conversion ([d195ed5](https://github.com/gnuzd/lazypock/commit/d195ed5dbc455c1f17adf6b6eb10c3eff7073fc5))
* **import:** make PocketBase JSON exports import cleanly + honor deleteMissing for fields ([b0b5429](https://github.com/gnuzd/lazypock/commit/b0b5429d9b4aa3cec63aab1ed908f1927cdda104))
* **import:** PocketBase JSON exports import cleanly + deleteMissing honors fields ([6830c4b](https://github.com/gnuzd/lazypock/commit/6830c4ba243007ec57312c82836f77d036b80450))

## [0.4.1](https://github.com/gnuzd/lazypock/compare/v0.4.0...v0.4.1) (2026-08-22)


### Bug Fixes

* **migrations:** only treat numeric-prefixed files as migration versions ([83bff32](https://github.com/gnuzd/lazypock/commit/83bff3267c90c2abe175db1359162e5293caf181))
* **migrations:** only treat numeric-prefixed files as migration versions ([1ee7beb](https://github.com/gnuzd/lazypock/commit/1ee7beb0ed7dcae221b0ceda627dc528c4cc20c8))

## [0.4.0](https://github.com/gnuzd/lazypock/compare/v0.3.3...v0.4.0) (2026-08-22)


### Features

* **migrations:** run bundled system migrations from inside the binary ([7cbfcca](https://github.com/gnuzd/lazypock/commit/7cbfcca46ecaaeae91190e335d3128058099748d))
* **migrations:** run bundled system migrations from inside the binary ([fcb4e5e](https://github.com/gnuzd/lazypock/commit/fcb4e5ecc4e92a2211a5e869035fdb08272ec137))

## [0.3.3](https://github.com/gnuzd/lazypock/compare/v0.3.2...v0.3.3) (2026-08-22)


### Bug Fixes

* **studio:** document /api proxy in shared client ([755dfea](https://github.com/gnuzd/lazypock/commit/755dfea79cd4e56d1b25b6fe47fef15589ee33b2))

## [0.3.2](https://github.com/gnuzd/lazypock/compare/v0.3.1...v0.3.2) (2026-08-22)


### Bug Fixes

* **core:** reference app-context module in moduledoc ([5925059](https://github.com/gnuzd/lazypock/commit/5925059da15b531294d978ea6cfe1477ad9d9e87))
* **core:** reference app-context module in moduledoc ([725fcb1](https://github.com/gnuzd/lazypock/commit/725fcb13ddfb3c6e52c2df0367277daebf75df4e))

## [0.3.1](https://github.com/gnuzd/lazypock/compare/v0.3.0...v0.3.1) (2026-08-22)


### Bug Fixes

* **core:** document canonical app-context accessor for hook handlers ([38a5a8d](https://github.com/gnuzd/lazypock/commit/38a5a8de711cac86a2b7018f038460a1fabf7e50))
* **core:** document canonical app-context accessor for hook handlers ([c7ceaeb](https://github.com/gnuzd/lazypock/commit/c7ceaeb5ed7900072d819508cdab6fad20bb7708))

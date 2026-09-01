# Changelog

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

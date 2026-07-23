<script>
  import { slugify } from "../lib/fieldTypes.js";
  import { store } from "../lib/store.svelte.js";
  import {
    GripVertical, Text, FileCode, Hash, ToggleLeft, Mail, Link, Calendar,
    CalendarCheck, ListChecks, Braces, Image, GitBranch, MapPin, Lock,
    Info, MoreHorizontal, Settings
  } from "@lucide/svelte";

  const typeIcons = {
    text: Text,
    editor: FileCode,
    number: Hash,
    bool: ToggleLeft,
    email: Mail,
    url: Link,
    date: Calendar,
    autodate: CalendarCheck,
    select: ListChecks,
    json: Braces,
    file: Image,
    relation: GitBranch,
    geoPoint: MapPin,
    password: Lock,
  };

  let {
    field = $bindable(),
    fieldIndex = 0,
  } = $props();

  let open = $state(false);

  function toggle() {
    open = !open;
  }

  function autofocus(node, shouldFocus) {
    if (!shouldFocus) return;
    setTimeout(() => {
      node?.select();
      delete field.__focus;
    }, 0);
  }


</script>

{#if !field["@toDelete"]}
  <details class="accordion record-field-settings field-type-{field.type}" name="collection_field" open={open} ontoggle={(e) => { open = e.target.open; }}>
    <summary tabindex="-1" onclick={(e) => e.preventDefault()} onkeydown={(e) => e.stopPropagation()}>
      <div class="sort-handle" draggable="true">
        <span class="icon"><GripVertical size={16} /></span>
      </div>
      <header class="header-fields" onclick={(e) => { e.stopPropagation(); e.preventDefault(); }}>
        <div class="fields">
          <label class="field addon" class:system={field.system}>
            {#each [typeIcons[field.type] || Text] as Cmp}
              <Cmp size={18} />
            {/each}
          </label>
          <div class="field prop-name">
            <input
              type="text"
              class="input input-sm inline-error"
              placeholder="Field name*"
              value={field.name || ""}
              disabled={field.system}
              oninput={(e) => { field.name = slugify(e.target.value); }}
              spellcheck="false"
              required
              use:autofocus={field.__focus}
            />
            <div class="field-labels">
              {#if field.required}
                <span class="label success">Required</span>
              {/if}
              {#if field.hidden}
                <span class="label danger">Hidden</span>
              {:else if field.presentable}
                <span class="label info">Presentable</span>
              {/if}
            </div>
          </div>

          <!-- Header extras -->
          {#if field.type === "autodate"}
            {@render FieldAutodateHeader({ field })}
          {:else if field.type === "select"}
            {@render FieldSelectHeader({ field, fieldIndex })}
          {:else if field.type === "relation"}
            {@render FieldRelationHeader({ field })}

          {:else if field.type === "file"}
            {@render FieldFileHeader({ field })}
          {/if}
        </div>
      </header>
      <button class="btn btn-sm circle transparent secondary" title="Field options" onclick={toggle}>
        <span class="icon"><Settings size={16} /></span>
      </button>
    </summary>

    <div class="field-settings-content">
      {@render FieldTypeContent()}
    </div>

    <footer class="record-field-settings-footer">
      {@render FieldTypeFooter()}

      <div class="field prop-presentable">
        <input type="checkbox" class="sm" id="presentable_{fieldIndex}" checked={field.presentable} onchange={(e) => field.presentable = e.target.checked} disabled={field.hidden} />
        <label for="presentable_{fieldIndex}">
          <span class="txt">Presentable</span>
          <span class="icon"><Info size={14} /></span>
        </label>
      </div>
      <div class="field prop-hidden">
        <input type="checkbox" class="sm" id="hidden_{fieldIndex}" checked={field.hidden} onchange={(e) => field.hidden = e.target.checked} />
        <label for="hidden_{fieldIndex}">
          <span class="txt">Hidden</span>
          <span class="icon"><Info size={14} /></span>
        </label>
      </div>
      <button class="btn btn-sm circle transparent secondary more-btn m-l-auto" title="More options">
        <span class="icon"><MoreHorizontal size={16} /></span>
      </button>
    </footer>
  </details>
{/if}

<!-- ===== Autodate header ===== -->
{#snippet FieldAutodateHeader({ field })}
  {@const opt = field.onCreate && field.onUpdate ? "create/update" : field.onUpdate ? "update" : "create"}
  <div class="field header-select autodate-select">
    <select class="input input-sm"
      value={opt}
      onchange={(e) => {
        const v = e.target.value;
        field.onCreate = v === "create" || v === "create/update";
        field.onUpdate = v === "update" || v === "create/update";
      }}
    >
      <option value="create">Create</option>
      <option value="update">Update</option>
      <option value="create/update">Create/Update</option>
    </select>
  </div>
{/snippet}

<!-- ===== Select header ===== -->
{#snippet FieldSelectHeader({ field, fieldIndex })}
  <div class="field header-select field-select-choices-input">
    <input type="text" placeholder="Add choices*" class="input input-sm txt-left"
      value={field.values?.join(" \u2022 ") || ""}
      onclick={() => {
        field._showChoices = true;
        field._choicesInput = (field.values || []).join("\n");
      }}
      readonly
    />
  </div>
  {#if field._showChoices}
    <div class="dropdown nowrap field-select-choices-dropdown open" style="position:absolute;z-index:10;top:100%;left:0;min-width:280px;">
      <div class="field-help m-t-0" style="font-size:0.9em">New-line separated choices:</div>
      <div class="field" style="min-height:100px;">
        <!-- svelte-ignore a11y-autofocus -->
        <textarea id={"choices-" + fieldIndex} class="autoexpand" style="min-height:80px;"
          bind:value={field._choicesInput}
          oninput={() => {
            field.values = (field._choicesInput || "").split("\n").filter(v => v.trim());
            if (field.maxSelect > field.values.length) {
              field.maxSelect = field.values.length;
            }
          }}
          onblur={() => { setTimeout(() => field._showChoices = false, 200); }}
        ></textarea>
      </div>
    </div>
  {/if}
  <div class="field header-select single-multiple-select">
    <select class="input input-sm"
      value={field.maxSelect > 1 ? "multiple" : "single"}
      onchange={(e) => { field.maxSelect = e.target.value === "multiple" ? (field.values?.length || 2) : 1; }}
    >
      <option value="single">Single</option>
      <option value="multiple">Multiple</option>
    </select>
  </div>
{/snippet}

<!-- ===== Relation header ===== -->
{#snippet FieldRelationHeader({ field })}


  <div class="field header-select collections-select">
    <select class="input input-sm"
      value={field.collectionId || ""}
      onchange={(e) => field.collectionId = e.target.value}
    >
      <option value="" disabled>Select collection*</option>
      {#each store.collections.filter(c => c.type !== "view") as coll}

        <option value={coll.id}>{coll.name}</option>
      {/each}
    </select>
  </div>
  <div class="field header-select single-multiple-select">
    <select class="input input-sm"
      value={field.maxSelect > 1 ? "multiple" : "single"}
      onchange={(e) => { field.maxSelect = e.target.value === "multiple" ? 10 : 1; }}
    >
      <option value="single">Single</option>
      <option value="multiple">Multiple</option>
    </select>
  </div>
{/snippet}

<!-- ===== File header (Single/Multiple) ===== -->
{#snippet FieldFileHeader({ field })}
  <div class="field header-select single-multiple-select">
    <select class="input input-sm"
      value={field.maxSelect > 1 ? "multiple" : "single"}
      onchange={(e) => { field.maxSelect = e.target.value === "multiple" ? (field.maxSelect || 10) : 1; }}
    >
      <option value="single">Single</option>
      <option value="multiple">Multiple</option>
    </select>
  </div>
{/snippet}

<!-- ===== Type-specific content ===== -->
{#snippet FieldTypeContent()}
  {#if field.type === "text"}
    <div class="grid-sm">
      <div class="col-sm-6">
        <div class="field">
          <label for="field_{fieldIndex}_min">
            <span class="txt">Min length</span>
            <i class="ri-information-line link-hint" title="Clear the field or set it to 0 for no limit."></i>
          </label>
          <input type="number" id="field_{fieldIndex}_min" step="1" min="0" placeholder="No min limit"
            value={field.min || ""}
            oninput={(e) => { if (e.target.value.length > 1 && e.target.value[0] == "0") return; field.min = parseInt(e.target.value, 10); }}
          />
        </div>
      </div>
      <div class="col-sm-6">
        <div class="field">
          <label for="field_{fieldIndex}_max">
            <span class="txt">Max length</span>
            <i class="ri-information-line link-hint" title="Clear the field or set it to 0 to fallback to the default limit."></i>
          </label>
          <input type="number" id="field_{fieldIndex}_max" step="1" min={field.min || 0} placeholder="Default to max 5000 characters"
            value={field.max || ""}
            oninput={(e) => { if (e.target.value.length > 1 && e.target.value[0] == "0") return; field.max = parseInt(e.target.value, 10); }}
          />
        </div>
      </div>
      <div class="col-sm-6">
        <div class="field">
          <label for="field_{fieldIndex}_pattern">
            <span class="txt">Validation pattern</span>
            {#if field.primaryKey}
              <i class="ri-information-line link-hint" title="All record ids have forbidden characters and unique case-insensitive (ASCII) validations in addition to the user defined regex pattern."></i>
            {/if}
          </label>
          <input type="text" id="field_{fieldIndex}_pattern" placeholder="e.g. ^[a-z0-9]+$"
            value={field.pattern || ""}
            oninput={(e) => field.pattern = e.target.value}
          />
        </div>
        <div class="field-help">Ex. <code>^[a-z0-9]+$</code></div>
      </div>

      <div class="col-sm-6">
        <div class="field">
          <label for="field_{fieldIndex}_autogenerate">
            <span class="txt">Autogenerate pattern</span>
            <i class="ri-information-line link-hint" title="Set and autogenerate text matching the pattern on missing record create value."></i>
          </label>
          <input type="text" id="field_{fieldIndex}_autogenerate" placeholder="e.g. [a-z0-9]{30}"
            value={field.autogeneratePattern || ""}
            oninput={(e) => field.autogeneratePattern = e.target.value}
          />
        </div>
        <div class="field-help">Ex. <code>[a-z0-9]{30}</code></div>
      </div>
      <div class="col-sm-12">
        <div class="field">
          <label for="field_{fieldIndex}_help">
            <span class="txt">Help text</span>
          </label>
          <input type="text" id="field_{fieldIndex}_help"
            value={field.help || ""}
            oninput={(e) => field.help = e.target.value}
          />
        </div>
      </div>
    </div>
  {:else if field.type === "number"}
    <div class="grid-sm">
      <div class="col-sm-6">
        <div class="field">
          <label for="field_{fieldIndex}_min"><span class="txt">Min</span></label>
          <input type="number" id="field_{fieldIndex}_min"
            value={typeof field.min === "number" ? field.min : ""}
            oninput={(e) => { if (!e.target.value) { field.min = null; return; } field.min = Number(e.target.value); }}
          />
        </div>
      </div>
      <div class="col-sm-6">
        <div class="field">
          <label for="field_{fieldIndex}_max"><span class="txt">Max</span></label>
          <input type="number" id="field_{fieldIndex}_max" min={field.min}
            value={typeof field.max === "number" ? field.max : ""}
            oninput={(e) => { if (!e.target.value) { field.max = null; return; } field.max = Number(e.target.value); }}
          />
        </div>
      </div>
      <div class="col-sm-12">
        <div class="field">
          <label for="field_{fieldIndex}_help"><span class="txt">Help text</span></label>
          <input type="text" id="field_{fieldIndex}_help"
            value={field.help || ""}
            oninput={(e) => field.help = e.target.value}
          />
        </div>
      </div>
    </div>
  {:else if field.type === "bool"}
    <div class="grid-sm">
      <div class="col-sm-12">
        <div class="field">
          <label for="field_{fieldIndex}_help"><span class="txt">Help text</span></label>
          <input type="text" id="field_{fieldIndex}_help"
            value={field.help || ""}
            oninput={(e) => field.help = e.target.value}
          />
        </div>
      </div>
    </div>
  {:else if field.type === "editor"}
    <div class="grid-sm">
      <div class="col-sm-12">
        <div class="field">
          <label for="field_{fieldIndex}_maxSize">
            <span class="txt">Max size </span><small>(bytes)</small>
          </label>
          <input type="number" id="field_{fieldIndex}_maxSize" step="1" min="0" placeholder="Default to max ~5MB"
            value={field.maxSize || ""}
            oninput={(e) => { if (e.target.value.length > 1 && e.target.value[0] == "0") return; field.maxSize = parseInt(e.target.value, 10); }}
          />
        </div>
      </div>
      <div class="col-sm-12">
        <div class="field">
          <label for="field_{fieldIndex}_help"><span class="txt">Help text</span></label>
          <input type="text" id="field_{fieldIndex}_help"
            value={field.help || ""}
            oninput={(e) => field.help = e.target.value}
          />
        </div>
      </div>
    </div>
  {:else if field.type === "email"}
    <div class="grid-sm">
      <div class="col-sm-6">
        <div class="field">
          <label for="field_{fieldIndex}_except"><span class="txt">Except domains</span>
            <i class="ri-information-line link-hint" title='List of domains that are NOT allowed.\nThis field is disabled if "Only domains" is set.'></i>
          </label>
          <input type="text" id="field_{fieldIndex}_except" disabled={field.onlyDomains?.length}
            value={field.exceptDomains?.join(", ") || ""}
            onchange={(e) => field.exceptDomains = e.target.value.split(",").map(s => s.trim()).filter(Boolean)}
          />
        </div>
        <div class="field-help">Use comma as separator.</div>
      </div>

      <div class="col-sm-6">
        <div class="field">
          <label for="field_{fieldIndex}_only"><span class="txt">Only domains</span>
            <i class="ri-information-line link-hint" title='List of domains that are ONLY allowed.\nThis field is disabled if "Except domains" is set.'></i>
          </label>
          <input type="text" id="field_{fieldIndex}_only" disabled={field.exceptDomains?.length}
            value={field.onlyDomains?.join(", ") || ""}
            onchange={(e) => field.onlyDomains = e.target.value.split(",").map(s => s.trim()).filter(Boolean)}
          />
        </div>
        <div class="field-help">Use comma as separator.</div>
      </div>
      <div class="col-sm-12">
        <div class="field">
          <label for="field_{fieldIndex}_help"><span class="txt">Help text</span></label>
          <input type="text" id="field_{fieldIndex}_help"
            value={field.help || ""}
            oninput={(e) => field.help = e.target.value}
          />
        </div>
      </div>
    </div>
  {:else if field.type === "url"}
    <div class="grid-sm">
      <div class="col-sm-12">
        <div class="field">
          <label for="field_{fieldIndex}_help"><span class="txt">Help text</span></label>
          <input type="text" id="field_{fieldIndex}_help"
            value={field.help || ""}
            oninput={(e) => field.help = e.target.value}
          />
        </div>
      </div>
    </div>
  {:else if field.type === "date"}
    <div class="grid-sm">
      <div class="col-sm-6">
        <div class="field">
          <label for="field_{fieldIndex}_minDate"><span class="txt">Min date (Local)</span></label>
          <input type="datetime-local" id="field_{fieldIndex}_minDate" step="1"
            value={field.min || ""}
            onchange={(e) => field.min = e.target.value}
          />
        </div>
      </div>
      <div class="col-sm-6">
        <div class="field">
          <label for="field_{fieldIndex}_maxDate"><span class="txt">Max date (Local)</span></label>
          <input type="datetime-local" id="field_{fieldIndex}_maxDate" step="1"
            value={field.max || ""}
            onchange={(e) => field.max = e.target.value}
          />
        </div>
      </div>
      <div class="col-sm-12">
        <div class="field">
          <label for="field_{fieldIndex}_help"><span class="txt">Help text</span></label>
          <input type="text" id="field_{fieldIndex}_help"
            value={field.help || ""}
            oninput={(e) => field.help = e.target.value}
          />
        </div>
      </div>
    </div>
  {:else if field.type === "select"}
    <div class="grid-sm">
      <div class="col-sm-12" hidden={field.maxSelect < 2}>
        <div class="field">
          <label for="field_{fieldIndex}_maxSelect"><span class="txt">Max select</span></label>
          <input type="number" id="field_{fieldIndex}_maxSelect" step="1" min="2" max={field.values?.length || 2} placeholder="Default to single"
            value={field.maxSelect || ""}
            onchange={(e) => { const v = parseInt(e.target.value, 10); field.maxSelect = v > 1 ? v : 1; }}
          />
        </div>
      </div>
      <div class="col-sm-12">
        <div class="field">
          <label for="field_{fieldIndex}_help"><span class="txt">Help text</span></label>
          <input type="text" id="field_{fieldIndex}_help"
            value={field.help || ""}
            oninput={(e) => field.help = e.target.value}
          />
        </div>
      </div>
    </div>
  {:else if field.type === "json"}
    <div class="grid-sm">
      <div class="col-sm-12">
        <div class="field">
          <label for="field_{fieldIndex}_maxSize">
            <span class="txt">Max size </span><small>(bytes)</small>
          </label>
          <input type="number" id="field_{fieldIndex}_maxSize" step="1" min="0" placeholder="Default to max ~1MB"
            value={field.maxSize || ""}
            oninput={(e) => { if (e.target.value.length > 1 && e.target.value[0] == "0") return; field.maxSize = parseInt(e.target.value, 10); }}
          />
        </div>
      </div>
      <div class="col-sm-12">
        <div class="field">
          <label for="field_{fieldIndex}_help"><span class="txt">Help text</span></label>
          <input type="text" id="field_{fieldIndex}_help"
            value={field.help || ""}
            oninput={(e) => field.help = e.target.value}
          />
        </div>
      </div>
      <div class="col-sm-12">
        <button type="button" class="btn sm secondary transparent">
          <span class="txt">String value normalizations</span>
          <i class="ri-arrow-down-s-line"></i>
        </button>
      </div>
    </div>
  {:else if field.type === "file"}
    <div class="grid-sm">
      <div class="col-sm-12">
        <div class="field">
          <label for="field_{fieldIndex}_mimeTypes">
            <span class="txt">Allowed mime types</span>
            <i class="ri-information-line link-hint" title="Allow files ONLY with the listed mime types. Leave empty for no restriction."></i>
          </label>
          <input type="text" id="field_{fieldIndex}_mimeTypes" placeholder="No restriction"
            value={field.mimeTypes?.join(", ") || ""}
            onchange={(e) => field.mimeTypes = e.target.value.split(",").map(s => s.trim()).filter(Boolean)}
          />
        </div>
      </div>
      <div class="col-sm-9">
        <div class="field">
          <label for="field_{fieldIndex}_thumbs">
            <span class="txt">Thumb sizes</span>
            <i class="ri-information-line link-hint" title="List of additional thumb sizes for image files, along with the default thumb size of 100x100. The thumbs are generated lazily on first access."></i>
          </label>
          <input type="text" id="field_{fieldIndex}_thumbs" placeholder="e.g. 50x50, 480x720"
            value={field.thumbs?.join(", ") || ""}
            onchange={(e) => field.thumbs = e.target.value.split(",").map(s => s.trim()).filter(Boolean)}
          />
        </div>
        <div class="field-help"><span class="txt m-r-5">Use comma as separator.</span></div>
      </div>
      <div class="col-sm-3">
        <div class="field">
          <label for="field_{fieldIndex}_maxSize"><span class="txt">Max size</span></label>
          <input type="number" id="field_{fieldIndex}_maxSize" step="1" min="0" placeholder="~5MB default"
            value={field.maxSize || ""}
            oninput={(e) => { if (e.target.value.length > 1 && e.target.value[0] == "0") return; field.maxSize = parseInt(e.target.value, 10); }}
          />
        </div>
        <div class="field-help">In bytes.</div>
      </div>
      <div class="col-sm-3" hidden={field.maxSelect < 2}>
        <div class="field">
          <label for="field_{fieldIndex}_maxSelect"><span class="txt">Max select</span></label>
          <input type="number" id="field_{fieldIndex}_maxSelect" step="1" min="2" placeholder="Default to single"
            value={field.maxSelect || ""}
            onchange={(e) => { const v = parseInt(e.target.value, 10); field.maxSelect = v > 1 ? v : 1; }}
          />
        </div>
      </div>
      <div class="col-sm-12">
        <div class="field m-t-5 m-b-5">
          <input type="checkbox" class="switch" id="field_{fieldIndex}_protected"
            checked={field.protected} onchange={(e) => field.protected = e.target.checked}
          />
          <label for="field_{fieldIndex}_protected">
            <span class="txt">Protected</span>
            <small class="txt-hint">File download requests will need to satisfy the View API rule.</small>
          </label>
        </div>
      </div>
      <div class="col-sm-12">
        <div class="field">
          <label for="field_{fieldIndex}_help"><span class="txt">Help text</span></label>
          <input type="text" id="field_{fieldIndex}_help"
            value={field.help || ""}
            oninput={(e) => field.help = e.target.value}
          />
        </div>
      </div>
    </div>
  {:else if field.type === "relation"}
    <div class="grid-sm">
      <div class="col-sm-6" hidden={field.maxSelect < 2}>
        <div class="field">
          <label for="field_{fieldIndex}_minSelect"><span class="txt">Min select</span></label>
          <input type="number" id="field_{fieldIndex}_minSelect" step="1" min="0" placeholder="No min limit"
            value={field.minSelect || ""}
            onchange={(e) => field.minSelect = parseInt(e.target.value, 10)}
          />
        </div>
      </div>
      <div class="col-sm-6" hidden={field.maxSelect < 2}>
        <div class="field">
          <label for="field_{fieldIndex}_maxSelect"><span class="txt">Max select</span></label>
          <input type="number" id="field_{fieldIndex}_maxSelect" step="1" min={field.minSelect || 2} placeholder="Default to single"
            value={field.maxSelect || ""}
            onchange={(e) => { const v = parseInt(e.target.value, 10); field.maxSelect = v > 1 ? v : 1; }}
          />
        </div>
      </div>
      <div class="col-sm-12">
        <div class="field">
          <label for="field_{fieldIndex}_cascade"><span class="txt">Cascade delete</span></label>
          <select class="input input-sm"
            value={field.cascadeDelete ? "true" : "false"}
            onchange={(e) => field.cascadeDelete = e.target.value === "true"}
          >
            <option value="false">False</option>
            <option value="true">True</option>
          </select>
        </div>
      </div>
      <div class="col-sm-12">
        <div class="field">
          <label for="field_{fieldIndex}_help"><span class="txt">Help text</span></label>
          <input type="text" id="field_{fieldIndex}_help"
            value={field.help || ""}
            oninput={(e) => field.help = e.target.value}
          />
        </div>
      </div>
    </div>
  {:else if field.type === "geoPoint"}
    <div class="grid-sm">
      <div class="col-sm-12">
        <div class="field">
          <label for="field_{fieldIndex}_help"><span class="txt">Help text</span></label>
          <input type="text" id="field_{fieldIndex}_help"
            value={field.help || ""}
            oninput={(e) => field.help = e.target.value}
          />
        </div>
      </div>
    </div>
  {:else if field.type === "password"}
    <div class="grid-sm">
      <div class="col-sm-6">
        <div class="field">
          <label for="field_{fieldIndex}_min">
            <span class="txt">Min length</span>
            <i class="ri-information-line link-hint" title="Clear the field or set it to 0 for no limit."></i>
          </label>
          <input type="number" id="field_{fieldIndex}_min" step="1" min="0" max="71" placeholder="No min limit"
            value={field.min || ""}
            oninput={(e) => { if (e.target.value.length > 1 && e.target.value[0] == "0") return; field.min = parseInt(e.target.value, 10); }}
          />
        </div>
      </div>
      <div class="col-sm-6">
        <div class="field">
          <label for="field_{fieldIndex}_max">
            <span class="txt">Max length</span>
            <i class="ri-information-line link-hint" title="Clear the field or set it to 0 to fallback to the default limit (71)."></i>
          </label>
          <input type="number" id="field_{fieldIndex}_max" step="1" min={field.min || 0} max="71" placeholder="Up to 71 chars"
            value={field.max || ""}
            oninput={(e) => { if (e.target.value.length > 1 && e.target.value[0] == "0") return; field.max = parseInt(e.target.value, 10); }}
          />
        </div>
      </div>
      <div class="col-sm-6">
        <div class="field">
          <label for="field_{fieldIndex}_cost">
            <span class="txt">Bcrypt cost</span>
            <i class="ri-information-line link-hint" title="Clear the field or set it to 0 to fallback to the default (10)."></i>
          </label>
          <input type="number" id="field_{fieldIndex}_cost" step="1" min="4" max="31" placeholder="Default to 10"
            value={field.cost || ""}
            oninput={(e) => { if (e.target.value.length > 1 && e.target.value[0] == "0") return; field.cost = parseInt(e.target.value, 10); }}
          />
        </div>
      </div>
      <div class="col-sm-6">
        <div class="field">
          <label for="field_{fieldIndex}_pattern">
            <span class="txt">Validation pattern</span>
          </label>
          <input type="text" id="field_{fieldIndex}_pattern" placeholder="ex. ^\w+$"
            value={field.pattern || ""}
            oninput={(e) => field.pattern = e.target.value}
          />
        </div>
      </div>
      <div class="col-sm-12">
        <div class="field">
          <label for="field_{fieldIndex}_help"><span class="txt">Help text</span></label>
          <input type="text" id="field_{fieldIndex}_help"
            value={field.help || ""}
            oninput={(e) => field.help = e.target.value}
          />
        </div>
      </div>
    </div>
  {/if}
{/snippet}

<!-- ===== Type-specific footer ===== -->
{#snippet FieldTypeFooter()}
  {#if field.type === "number"}
    <div class="field">
      <input type="checkbox" class="sm" id="field_{fieldIndex}_onlyInt" checked={field.onlyInt} onchange={(e) => field.onlyInt = e.target.checked} />
      <label for="field_{fieldIndex}_onlyInt">
        <span class="txt">No decimals</span>
        <i class="ri-information-line link-hint" title="Existing decimal numbers will not be affected."></i>
      </label>
    </div>
  {/if}
  {#if field.type === "editor"}
    <div class="field">
      <input type="checkbox" class="sm" id="field_{fieldIndex}_convertURLs" checked={field.convertURLs} onchange={(e) => field.convertURLs = e.target.checked} />
      <label for="field_{fieldIndex}_convertURLs">
        <span class="txt">Strip URLs domain</span>
        <i class="ri-information-line link-hint" title="This could help making the editor content more portable between environments since there will be no local base url to replace."></i>
      </label>
    </div>
  {/if}
  {#if field.type === "text" || field.type === "email" || field.type === "url" || field.type === "select" || field.type === "relation" || field.type === "editor" || field.type === "password"}
    <div class="field">
      <input type="checkbox" class="sm" id="field_{fieldIndex}_required" checked={field.required} onchange={(e) => field.required = e.target.checked} />
      <label for="field_{fieldIndex}_required">
        <span class="txt">Required</span>
        {#if field.type === "bool"}
          <small class="txt-hint">(=true)</small>
          <i class="ri-information-line link-hint" title="Requires the field value to be true."></i>
        {:else if field.type === "number"}
          <small class="txt-hint">(!=0)</small>
          <i class="ri-information-line link-hint" title="Requires the field value to be not 0."></i>
        {:else if field.type === "select"}
          <small class="txt-hint">{field.maxSelect > 1 ? "(!=[])" : "(!='')"}</small>
          <i class="ri-information-line link-hint" title="Requires the field value to be nonempty {field.maxSelect > 1 ? 'array' : 'string'}."></i>
        {:else if field.type === "relation"}
          <small class="txt-hint">{field.maxSelect > 1 ? "(!=[])" : "(!='')"}</small>
          <i class="ri-information-line link-hint" title="Requires the field value to be nonempty {field.maxSelect > 1 ? 'array' : 'string'}."></i>
        {:else}
          <small class="txt-hint">(!='')</small>
          <i class="ri-information-line link-hint" title="Requires the field value to be nonempty string."></i>
        {/if}
      </label>
    </div>
  {/if}
{/snippet}

<style>
  /* PocketBase exact: sort handle absolutely positioned left, hidden, appears on hover */
  .sort-handle {
    position: absolute;
    z-index: 2;
    top: 0;
    left: -30px;
    width: 30px;
    height: 100%;
    display: flex;
    align-items: center;
    padding-left: 7px;
    flex-shrink: 0;
    cursor: grab;
    color: var(--color-base-hint);
    opacity: 0;
    transform: translateX(3px);
    transition:
      color var(--animation-speed),
      opacity var(--animation-speed),
      transform var(--animation-speed);
  }
  .sort-handle:hover,
  .sort-handle:active {
    opacity: 1;
    transform: translateX(0);
    color: var(--color-base-content);
  }
  :global(.record-field-settings:hover) .sort-handle {
    opacity: 1;
    transform: translateX(0);
  }

  /* PocketBase exact: summary padding 4px, background inputColor */
  :global(.record-field-settings > summary) {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    padding: 4px;
    min-height: 0;
    background: var(--input-color);
    container-type: inline-size;
  }

  /* PocketBase exact: header-fields creates auto delimiters */
  .header-fields {
    display: flex;
    gap: var(--spacing-sm);
    width: 100%;
    align-content: center;
    align-items: stretch;
  }
  .header-fields > :global(*) {
    position: relative;
  }
  .header-fields > :global(*)::before {
    content: "";
    position: absolute;
    top: -4px;
    bottom: -4px;
    right: -5px;
    width: 1px;
    height: auto;
    min-height: 0;
    flex-shrink: 0;
    background: var(--color-base-400);
  }

  /* PocketBase exact: field addon in header */
  .field.addon {
    display: flex;
    align-items: center;
    padding: 0;
  }
  .field.addon :global(svg) {
    opacity: 0.6;
  }

  /* PocketBase exact: field name input */
  .field.prop-name {
    flex: 1;
    min-width: 0;
    position: relative;
  }
  .field.prop-name :global(.input) {
    width: 100%;
    font-size: var(--font-size-sm);
    padding: 4px 8px;
  }

  /* PocketBase exact: labels positioned absolute top-right */
  .field-labels {
    position: absolute;
    z-index: 2;
    display: flex;
    align-items: center;
    top: 2px;
    right: 2px;
    gap: 2px;
    pointer-events: none;
    user-select: none;
  }
  .field-labels :global(.label) {
    font-size: 0.7em;
    padding: 2px 3px;
    border-radius: 2px;
    font-weight: 500;
    min-height: 0;
    line-height: 1.2;
  }

  /* PocketBase exact: content area */
  .field-settings-content {
    padding: 12px;
    border-top: 1px solid var(--color-base-400);
  }

  /* PocketBase exact: footer */
  .record-field-settings-footer {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--spacing-sm);
    margin-top: var(--spacing-sm);
    padding: 8px 12px;
    border-top: 1px solid var(--color-base-400);
    background: var(--color-base-200);
  }
  .record-field-settings-footer .field {
    display: flex;
    align-items: center;
    gap: 4px;
    margin: 0;
    width: auto;
  }
  .record-field-settings-footer .field label {
    cursor: pointer;
  }
  .record-field-settings-footer .field label .txt {
    font-weight: 500;
  }
  .record-field-settings-footer .field label :global(svg) {
    opacity: 0.4;
  }

  .m-l-auto {
    margin-left: auto;
  }

  /* PocketBase exact: header select widths */
  :global(.header-select) {
    max-width: 150px;
  }
  :global(.header-select select) {
    font-size: var(--font-size-xs);
    padding: 2px 6px;
  }
  :global(.header-select.field-select-choices-input),
  :global(.header-select.collections-select) {
    min-width: 200px;
    max-width: 200px;
  }

  .icon {
    display: inline-flex;
    align-items: center;
  }
</style>

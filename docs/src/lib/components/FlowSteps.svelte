<script lang="ts">
  // Vertical step "timeline" used for the request lifecycle and realtime
  // flow sections — numbered circles on a rail, content cards on the right.
  export interface FlowLine {
    text?: string;
    code?: string;
  }
  export interface FlowStep {
    title: string;
    lines: FlowLine[];
  }

  let { steps }: { steps: FlowStep[] } = $props();
</script>

<div class="relative">
  <!-- Continuous rail -->
  <div
    class="absolute left-[15px] top-3 bottom-3 w-0.5 rounded-full"
    style="background: color-mix(in oklch, var(--color-base-content) 18%, transparent)"
  ></div>

  <div class="space-y-4">
    {#each steps as step, i}
      <div class="relative pl-12">
        <!-- Number circle -->
        <div
          class="absolute left-0 top-0 flex h-8 w-8 items-center justify-center rounded-full border text-sm font-bold"
          style="background: var(--color-base-100); border-color: var(--color-primary); color: var(--color-primary)"
        >
          {i + 1}
        </div>

        <div class="rounded-box border border-base-300 p-4">
          <p class="font-semibold text-sm" style="color: var(--color-base-content)">
            {step.title}
          </p>
          <div class="mt-1.5 space-y-1.5">
            {#each step.lines as line}
              {#if line.code}
                <code class="flow-code">{line.code}</code>
              {/if}
              {#if line.text}
                <p class="text-[13px] leading-relaxed flow-text">{line.text}</p>
              {/if}
            {/each}
          </div>
        </div>
      </div>
    {/each}
  </div>
</div>

<style>
  .flow-code {
    display: inline-block;
    max-width: 100%;
    background: color-mix(in oklch, var(--color-base-content) 7%, transparent);
    color: color-mix(in oklch, var(--color-base-content) 85%, var(--color-primary) 15%);
    border-radius: var(--radius-field);
    font-family: ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, 'Liberation Mono',
      monospace;
    font-size: 12px;
    font-weight: 500;
    padding: 1px 6px;
    overflow-wrap: anywhere;
    vertical-align: middle;
  }

  .flow-text {
    color: color-mix(in oklch, var(--color-base-content) 75%, transparent);
  }
</style>

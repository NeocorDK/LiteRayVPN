<script lang="ts">
  let { checked = $bindable(false), onchange, disabled = false }: { checked?: boolean; onchange?: (v: boolean) => void; disabled?: boolean } =
    $props();
</script>

<label class="switch">
  <input type="checkbox" bind:checked {disabled} onchange={() => onchange?.(checked)} />
  <span></span>
</label>

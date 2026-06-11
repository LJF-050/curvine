<template>
  <article class="admin-metric-card">
    <span>{{ label }}</span>
    <strong>{{ value }}</strong>
    <div class="metric-foot">
      <small>{{ meta }}</small>
      <em :class="tone">{{ delta }}</em>
    </div>
  </article>
</template>

<script>
export default {
  name: 'MetricCard',
  props: {
    label: { type: String, required: true },
    value: { type: [String, Number], required: true },
    meta: { type: String, default: '' },
    delta: { type: String, default: '' },
    tone: { type: String, default: 'neutral' }
  }
}
</script>

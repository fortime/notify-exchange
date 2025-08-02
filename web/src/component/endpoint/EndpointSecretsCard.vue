<template>
  <div>
    <HttpEndpointSecretsCard
      v-if="transportServiceTypeCode === 'HTTP'"
      :endpoint-id="props.endpointId"
      class="mb-4"
    />
    <div v-else-if="transportServiceTypeCode === 'TELEGRAM'" />
    <div v-else-if="transportServiceTypeCode === ''" />
    <BCard v-else>
      <template #header>
        <div class="d-flex justify-content-between align-items-center">
          <span>Secrets</span>
        </div>
      </template>
      <p>Unknown transport service type {{ props.transportServiceType }}. No secret management available.</p>
    </BCard>
  </div>
</template>

<script setup>
import { computed } from 'vue'
import { BCard } from 'bootstrap-vue-next'
import HttpEndpointSecretsCard from './secret/HttpEndpointSecretsCard.vue'
import { TransportServiceType } from '../../service/enum'

const props = defineProps({
  transportServiceType: {
    type: Number,
    required: true
  },
  endpointId: {
    type: String,
    required: true
  }
})

const transportServiceTypeCode = computed(() => {
  if (typeof props.transportServiceType === 'undefined') {
      return '';
  }
  return TransportServiceType[props.transportServiceType]?.code
})
</script>

<template>
  <div>
    <BButton
      @click="goBack"
      variant="outline-secondary"
      class="mb-3"
      v-if="canGoBack"
    >
      <i class="bi bi-arrow-left" /> Back
    </BButton>
    <EndpointInfoCard
      :endpoint-id="endpointId"
      @endpoint-loaded="handleEndpointLoaded"
      class="mb-4"
    />
    <EndpointSecretsCard
      v-if="transportServiceType !== null"
      :transport-service-type="transportServiceType"
      :endpoint-id="endpointId"
      class="mb-4"
    />
    <EndpointSubscriptionsCard
      :endpoint-id="endpointId"
    />
  </div>
</template>

<script setup>
import { ref, computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { BButton } from 'bootstrap-vue-next'
import EndpointInfoCard from '../component/endpoint/EndpointInfoCard.vue'
import EndpointSecretsCard from '../component/endpoint/EndpointSecretsCard.vue'
import EndpointSubscriptionsCard from '../component/endpoint/EndpointSubscriptionsCard.vue'

const route = useRoute()
const router = useRouter()
const endpointId = ref(route.params.id)
const transportServiceType = ref(null)

const canGoBack = computed(() => {
  const history = JSON.parse(sessionStorage.getItem('navigationHistory') || '[]')
  return history.length > 0
})

function goBack() {
  let history = JSON.parse(sessionStorage.getItem('navigationHistory') || '[]')
  if (history.length > 0) {
    const lastRoute = history.pop()
    sessionStorage.setItem('navigationHistory', JSON.stringify(history))
    router.push(lastRoute)
  } else {
    router.push({ name: 'Home' })
  }
}

function handleEndpointLoaded(endpoint) {
  transportServiceType.value = endpoint.transport_service_type
}
</script>

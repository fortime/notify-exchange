<template>
  <BContainer fluid>
    <BButton
      @click="goBack"
      variant="outline-secondary"
      class="my-3"
      v-if="canGoBack"
    >
      <i class="bi bi-arrow-left" /> Back
    </BButton>
    <h2
      class="my-3"
      v-b-tooltip.hover
      :title="`ID: ${topicId}`"
    >
      Messages for Topic: {{ topicName }}
    </h2>
    <div v-if="messages.length > 0">
      <BTable
        :items="messages"
        :fields="fields"
        striped
        responsive
      >
        <template #cell(content)="{ item }">
          <pre class="message-content">{{ item.content }}</pre>
        </template>
        <template #cell(is_binary)="{ item }">
          <i
            v-if="item.is_binary"
            class="bi bi-check-lg text-success"
          />
        </template>
      </BTable>
    </div>
    <div
      v-else-if="!isLoading"
      class="text-center text-muted mt-5"
    >
      No messages found for this topic.
    </div>
    <div
      ref="loadTrigger"
      class="loading-trigger"
    >
      <div
        v-if="isLoading"
        class="text-center my-3"
      >
        <BSpinner label="Loading..." />
      </div>
      <div
        v-if="!hasMore"
        class="text-center my-3"
      >
        <p>No more data</p>
      </div>
    </div>
  </BContainer>
</template>

<script setup>
import { nextTick, ref, onMounted, onUnmounted, watch, computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { BContainer, BTable, BSpinner, BButton, vBTooltip } from 'bootstrap-vue-next'
import apiClient from '../service/api'
import { useToast } from 'vue-toast-notification'

const route = useRoute()
const router = useRouter()
const $toast = useToast()

const topicId = ref(route.params.topicId)
const topicName = ref('')
const messages = ref([])
const nextMaxOffset = ref(route.query.mo || null)
const isLoading = ref(false)
const hasMore = ref(true)
const isVisible = ref(false)
const loadTrigger = ref(null)

const fields = [
  { key: 'offset', label: 'Offset', tdClass: 'align-middle' },
  { key: 'content', label: 'Content', tdClass: 'align-middle' },
  { key: 'is_binary', label: 'Binary', tdClass: 'align-middle text-center' }
]

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
    // Fallback if history is empty for some reason
    router.push({ name: 'Home' })
  }
}

async function fetchTopicInfo() {
  try {
    const response = await apiClient.get(`/v1/topic/${topicId.value}`)
    topicName.value = response.data.data.topic.name
  } catch (error) {
    console.error('Failed to fetch topic info:', error)
    $toast.error('Failed to load topic details.')
  }
}

async function fetchMessages() {
  if (isLoading.value || !hasMore.value) return

  isLoading.value = true
  try {
    const params = {}
    const maxOffset = nextMaxOffset.value
    if (maxOffset) {
      params.max_offset = maxOffset
    }

    const response = await apiClient.get(`/v1/topic/${topicId.value}/message`, { params })
    const newMessages = response.data.data.messages || []

    messages.value.push(...newMessages)
    nextMaxOffset.value = response.data.data.next_max_offset
    hasMore.value = !!nextMaxOffset.value
    isLoading.value = false

    // after DOM update, check again
    await nextTick()
    if (isVisible.value) {
      fetchMessages()
    }
  } catch (error) {
    console.error('Failed to fetch messages:', error)
    $toast.error('Failed to fetch messages: ' + (error.response?.data?.message || error.message))
    hasMore.value = false
    isLoading.value = false
  }
}

const observer = new IntersectionObserver(
  entries => {
    isVisible.value = entries[0].isIntersecting;
    if (entries[0].isIntersecting) {
      fetchMessages()
    }
  },
  { threshold: 0.0 }
)

onMounted(() => {
  fetchTopicInfo()
  observer.observe(loadTrigger.value)
})

onUnmounted(() => {
  observer?.disconnect()
})

watch(() => route.params.topicId, (newTopicId) => {
  if (newTopicId) {
    topicId.value = newTopicId
    messages.value = []
    nextMaxOffset.value = route.query.mo || null
    hasMore.value = true
  }
})
</script>

<style scoped>
.message-content {
  white-space: pre-wrap;
  word-break: break-all;
  margin-bottom: 0;
}
</style>

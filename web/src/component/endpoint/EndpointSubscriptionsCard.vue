<template>
  <BCard>
    <template #header>
      <div class="d-flex justify-content-between align-items-center">
        <span>Subscriptions</span>
        <BButton
          size="sm"
          variant="outline-primary"
          @click="paginationRef?.fetchData()"
        >
          Refresh
        </BButton>
      </div>
    </template>
    <BTable
      v-if="subscriptions && subscriptions.length > 0"
      :items="subscriptions"
      :fields="fields"
      striped
      hover
      responsive
    >
      <template #cell(topic_name)="{ item }">
        <span
          v-b-tooltip.hover
          :title="item.topic_id"
        >{{ item.topic_name }}</span>
        <BButton
          size="sm"
          variant="link"
          @click="copyToClipboard(item.topic_id, `topic-id-${item.id}`)"
        >
          <i class="bi bi-clipboard" />
        </BButton>
        <span v-if="copied === `topic-id-${item.id}`">Copied!</span>
      </template>
      <template #cell(topic_code)="{ item }">
        <span>{{ item.topic_code }}</span>
        <BButton
          size="sm"
          variant="link"
          @click="copyToClipboard(item.topic_code, `topic-code-${item.id}`)"
        >
          <i class="bi bi-clipboard" />
        </BButton>
        <span v-if="copied === `topic-code-${item.id}`">Copied!</span>
      </template>
      <template #cell(committed_offset)="{ item }">
        {{ item.committed_offset }}
      </template>
      <template #cell(latest_offset)="{ item }">
        {{ item.latest_offset }}
      </template>
      <template #cell(created_at)="{ item }">
        {{ new Date(item.created_at).toLocaleString() }}
      </template>
      <template #cell(actions)="{ item }">
        <div class="d-flex align-items-center">
          <BButton
            class="mx-1"
            size="sm"
            variant="danger"
            @click="showDeleteModal(item)"
          >
            Unsubscribe
          </BButton>
        </div>
      </template>
    </BTable>
    <div
      v-else
      class="text-center text-muted"
    >
      No subscriptions have been added yet.
    </div>
    <hr>

    <AppPagination
      :url="isAdmin ? `/v1/admin/endpoint/${props.endpointId}/subscription` : `/v1/endpoint/${props.endpointId}/subscription`"
      data-key="subscriptions"
      @fetched="handleFetched"
      ref="paginationRef"
    />

    <!-- Delete Subscription Modal -->
    <BModal
      v-model="isDeleteModalVisible"
      title="Confirm Unsubscribe"
      @ok="handleDeleteSubscription"
      ok-title="Unsubscribe"
      @hidden="resetDeleteModal"
      ok-variant="danger"
    >
      <p>Are you sure you want to unsubscribe from topic <strong>{{ subscriptionToDelete?.topic_name }}</strong>?</p>
    </BModal>
  </BCard>
</template>

<script setup>
import { ref, computed } from 'vue'
import { BCard, BTable, BButton, BModal, vBTooltip } from 'bootstrap-vue-next'
import apiClient from '../../service/api'
import { useToast } from 'vue-toast-notification'
import AppPagination from '../AppPagination.vue'

const props = defineProps({
  endpointId: {
    type: String,
    required: true
  },
  isAdmin: {
    type: Boolean,
    default: false
  }
})

const $toast = useToast()
const paginationRef = ref(null)
const subscriptions = ref([])

const fields = computed(() => {
  const baseFields = [
    { key: 'topic_name', label: 'Topic Name', tdClass: 'align-middle' },
    { key: 'topic_code', label: 'Topic Code', tdClass: 'align-middle' },
    { key: 'committed_offset', label: 'Committed Offset', tdClass: 'align-middle' },
    { key: 'latest_offset', label: 'Latest Offset', tdClass: 'align-middle' },
    { key: 'created_at', label: 'Subscribed At', tdClass: 'align-middle' }
  ]
  if (!props.isAdmin) {
    baseFields.push({ key: 'actions', label: 'Actions', tdClass: 'align-middle' })
  }
  return baseFields
})

const handleFetched = (items) => {
  subscriptions.value = items
}

const copied = ref(null)
const copyToClipboard = (text, type) => {
  navigator.clipboard.writeText(text).then(() => {
    copied.value = type
    setTimeout(() => {
      copied.value = null
    }, 2000)
  })
}

// Delete Subscription Modal
const isDeleteModalVisible = ref(false)
const subscriptionToDelete = ref(null)

const showDeleteModal = (subscription) => {
  subscriptionToDelete.value = { ...subscription }
  isDeleteModalVisible.value = true
}

const handleDeleteSubscription = async () => {
  const subscription = subscriptionToDelete.value
  if (subscription) {
    try {
      await apiClient.delete(`/v1/endpoint/${props.endpointId}/subscription/${subscription.id}`)
      paginationRef.value?.fetchData()
      isDeleteModalVisible.value = false
      $toast.success(`Unsubscribed from topic '${subscription.topic_name}'.`)
    } catch (error) {
      console.error('Failed to unsubscribe:', error)
      $toast.error('Failed to unsubscribe: ' + (error.response?.data?.message || error.message))
    }
  }
}

const resetDeleteModal = () => {
  subscriptionToDelete.value = null
}
</script>

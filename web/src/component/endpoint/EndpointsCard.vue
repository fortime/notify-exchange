<template>
  <BCard>
    <template #header>
      <div class="d-flex justify-content-between align-items-center">
        <span>My Endpoints</span>
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
      v-if="endpoints && endpoints.length > 0"
      :items="endpoints"
      :fields="fields"
      striped
      hover
      responsive
    >
      <template #cell(name)="{ item }">
        <router-link
          :to="{ name: 'EndpointManager', params: { id: item.id } }"
          v-b-tooltip.hover
          :title="item.id"
        >
          {{ item.name }}
        </router-link>
        <BButton
          size="sm"
          variant="link"
          @click="copyToClipboard(item.id, `endpoint-id-${item.id}`)"
        >
          <i class="bi bi-clipboard" />
        </BButton>
        <span v-if="copied === `endpoint-id-${item.id}`">Copied!</span>
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
            Delete
          </BButton>
        </div>
      </template>
    </BTable>
    <div
      v-else
      class="text-center text-muted"
    >
      No endpoints have been created yet.
    </div>
    <hr>

    <AppPagination
      url="/v1/endpoint"
      data-key="endpoints"
      @fetched="handleFetched"
      ref="paginationRef"
    />

    <!-- Delete Endpoint Modal -->
    <BModal
      v-model="isDeleteModalVisible"
      title="Confirm Delete Endpoint"
      @ok="handleDeleteEndpoint"
      ok-title="Delete"
      @hidden="resetDeleteModal"
      ok-variant="danger"
    >
      <p>Are you sure you want to delete endpoint <strong>{{ endpointToDelete?.name }}</strong>?</p>
      <p class="text-danger">
        This action cannot be undone.
      </p>
    </BModal>
  </BCard>
</template>

<script setup>
import { ref } from 'vue'
import { BCard, BTable, BButton, BModal, vBTooltip } from 'bootstrap-vue-next'
import apiClient from '../../service/api'
import { useToast } from 'vue-toast-notification'
import AppPagination from '../AppPagination.vue'

const $toast = useToast()
const paginationRef = ref(null)
const endpoints = ref([])

const fields = [
  { key: 'name', label: 'Endpoint Name', tdClass: 'align-middle' },
  { key: 'created_at', label: 'Created At', tdClass: 'align-middle' },
  { key: 'actions', label: 'Actions', tdClass: 'align-middle' }
]

const handleFetched = (items) => {
  endpoints.value = items
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

// Delete Endpoint Modal
const isDeleteModalVisible = ref(false)
const endpointToDelete = ref(null)

const showDeleteModal = (endpoint) => {
  endpointToDelete.value = { ...endpoint }
  isDeleteModalVisible.value = true
}

const handleDeleteEndpoint = async () => {
  const endpoint = endpointToDelete.value
  if (endpoint) {
    try {
      await apiClient.delete(`/v1/endpoint/${endpoint.id}`)
      $toast.success(`Endpoint '${endpoint.name}' deleted successfully.`)
      paginationRef.value?.fetchData()
      isDeleteModalVisible.value = false
    } catch (error) {
      console.error('Failed to delete endpoint:', error)
      $toast.error('Failed to delete endpoint: ' + (error.response?.data?.message || error.message))
    }
  }
}

const resetDeleteModal = () => {
  endpointToDelete.value = null
}
</script>

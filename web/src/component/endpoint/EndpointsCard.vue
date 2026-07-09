<template>
  <BCard>
    <template #header>
      <div class="d-flex justify-content-between align-items-center">
        <span>My Endpoints</span>
        <div class="d-flex gap-2">
          <BButton
            v-if="httpTransportServices.length > 0"
            size="sm"
            variant="primary"
            @click="showCreateModal"
          >
            Create HTTP Endpoint
          </BButton>
          <BButton
            size="sm"
            variant="outline-primary"
            @click="refreshData"
          >
            Refresh
          </BButton>
        </div>
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

    <!-- Create HTTP Endpoint Modal -->
    <BModal
      v-model="isCreateModalVisible"
      title="Create HTTP Endpoint"
      @ok="handleCreateEndpoint"
      @hidden="resetCreateModal"
      ok-title="Create"
      :ok-disabled="!isCreateFormValid"
    >
      <BForm @submit.prevent="handleCreateEndpoint">
        <BFormGroup
          label="Transport Service"
          label-for="http-service-select"
          class="mb-3"
        >
          <BFormSelect
            id="http-service-select"
            v-model="newEndpoint.transport_service_id"
            :options="httpTransportServiceOptions"
            required
          />
        </BFormGroup>

        <BFormGroup
          label="Code"
          label-for="endpoint-code-input"
          class="mb-3"
        >
          <BFormInput
            id="endpoint-code-input"
            v-model="newEndpoint.code"
            required
            placeholder="endpoint-code"
          />
        </BFormGroup>

        <BFormGroup
          label="Name"
          label-for="endpoint-name-input"
          class="mb-3"
        >
          <BFormInput
            id="endpoint-name-input"
            v-model="newEndpoint.name"
            required
            placeholder="Endpoint name"
          />
        </BFormGroup>

        <BFormGroup
          label="Description"
          label-for="endpoint-description-textarea"
        >
          <BFormTextarea
            id="endpoint-description-textarea"
            v-model="newEndpoint.description"
            placeholder="Optional description"
            rows="3"
          />
        </BFormGroup>
      </BForm>
    </BModal>
  </BCard>
</template>

<script setup>
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { BCard, BTable, BButton, BModal, BForm, BFormGroup, BFormInput, BFormSelect, BFormTextarea, vBTooltip } from 'bootstrap-vue-next'
import apiClient from '../../service/api'
import { useToast } from 'vue-toast-notification'
import AppPagination from '../AppPagination.vue'

const $toast = useToast()
const router = useRouter()
const paginationRef = ref(null)
const endpoints = ref([])
const httpTransportServices = ref([])

const fields = [
  { key: 'name', label: 'Endpoint Name', tdClass: 'align-middle' },
  { key: 'created_at', label: 'Created At', tdClass: 'align-middle' },
  { key: 'actions', label: 'Actions', tdClass: 'align-middle' }
]

const handleFetched = (items) => {
  endpoints.value = items
}

const fetchHttpTransportServices = async () => {
  try {
    const response = await apiClient.get('/v1/endpoint/http-transport-service')
    httpTransportServices.value = response.data.data.transport_services
  } catch (error) {
    console.error('Failed to fetch HTTP transport services:', error)
  }
}

const refreshData = () => {
  fetchHttpTransportServices()
  paginationRef.value?.fetchData()
}

onMounted(() => {
  fetchHttpTransportServices()
})

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

// Create HTTP Endpoint Modal
const isCreateModalVisible = ref(false)
const newEndpoint = ref({
  transport_service_id: null,
  code: '',
  name: '',
  description: ''
})

const httpTransportServiceOptions = computed(() => {
  return httpTransportServices.value.map(service => ({
    value: service.id,
    text: service.name
  }))
})

const codeRegex = /^[a-zA-Z0-9-_]+$/
const isCreateFormValid = computed(() => {
  return !!newEndpoint.value.transport_service_id &&
    newEndpoint.value.code.length >= 4 &&
    newEndpoint.value.code.length <= 128 &&
    codeRegex.test(newEndpoint.value.code) &&
    newEndpoint.value.name.length >= 4 &&
    newEndpoint.value.name.length <= 128 &&
    newEndpoint.value.description.length <= 1024
})

const showCreateModal = () => {
  if (!newEndpoint.value.transport_service_id && httpTransportServices.value.length > 0) {
    newEndpoint.value.transport_service_id = httpTransportServices.value[0].id
  }
  isCreateModalVisible.value = true
}

const handleCreateEndpoint = async (event) => {
  if (event && event.preventDefault) event.preventDefault()
  if (!isCreateFormValid.value) return

  try {
    const response = await apiClient.post('/v1/endpoint/http', {
      transport_service_id: newEndpoint.value.transport_service_id,
      code: newEndpoint.value.code,
      name: newEndpoint.value.name,
      description: newEndpoint.value.description || null
    })
    const endpointId = response.data.data.id
    $toast.success('HTTP endpoint created successfully.')
    isCreateModalVisible.value = false
    paginationRef.value?.fetchData()
    router.push({ name: 'EndpointManager', params: { id: endpointId } })
  } catch (error) {
    console.error('Failed to create HTTP endpoint:', error)
    $toast.error('Failed to create HTTP endpoint: ' + (error.response?.data?.message || error.message))
  }
}

const resetCreateModal = () => {
  newEndpoint.value = {
    transport_service_id: httpTransportServices.value[0]?.id ?? null,
    code: '',
    name: '',
    description: ''
  }
}
</script>

<template>
  <BCard v-if="endpoint.id">
    <template #header>
      <div class="d-flex justify-content-between align-items-center">
        <span>Endpoint Information</span>
        <div class="d-flex flex-wrap gap-2">
          <BButton
            size="sm"
            variant="danger"
            @click="showDeleteModal"
            v-if="!isAdmin"
          >
            Delete Endpoint
          </BButton>
          <BButton
            size="sm"
            variant="outline-primary"
            @click="fetchEndpoint"
          >
            Refresh
          </BButton>
          <BButton
            size="sm"
            @click="toggleExpand"
          >
            {{ isExpanded ? 'Collapse' : 'Expand' }}
          </BButton>
        </div>
      </div>
    </template>
    <BRow class="mb-3 d-flex align-items-center">
      <BCol md="2">
        <strong>Name:</strong>
      </BCol>
      <BCol md="4">
        <div class="d-flex align-items-center">
          <span
            v-b-tooltip.hover
            :title="endpoint.id"
          >{{ endpoint.name }}</span>
          <BButton
            size="sm"
            variant="link"
            @click="copyToClipboard(endpoint.id, 'id')"
          >
            <i class="bi bi-clipboard" />
          </BButton>
          <span v-if="copied === 'id'">Copied!</span>
        </div>
      </BCol>
      <BCol md="2">
        <strong>Code:</strong>
      </BCol>
      <BCol md="4">
        <div class="d-flex align-items-center">
          <span>{{ endpoint.code }}</span>
          <BButton
            size="sm"
            variant="link"
            @click="copyToClipboard(endpoint.code, 'code')"
          >
            <i class="bi bi-clipboard" />
          </BButton>
          <span v-if="copied === 'code'">Copied!</span>
        </div>
      </BCol>
    </BRow>
    <div v-if="isExpanded">
      <BRow class="mb-3 d-flex align-items-center">
        <BCol md="2">
          <strong>Transport Service:</strong>
        </BCol>
        <BCol md="4">
          <div class="d-flex align-items-center">
            <span
              v-b-tooltip.hover
              :title="endpoint.transport_service_id"
            >{{ endpoint.transport_service_name }}</span>
            <BButton
              size="sm"
              variant="link"
              @click="copyToClipboard(endpoint.transport_service_id, 'transportServiceId')"
            >
              <i class="bi bi-clipboard" />
            </BButton>
            <span v-if="copied === 'transportServiceId'">Copied!</span>
          </div>
        </BCol>
        <BCol md="2">
          <strong>Type:</strong>
        </BCol>
        <BCol md="4">
          {{ TransportServiceType[endpoint.transport_service_type]?.name }}
        </BCol>
      </BRow>
      <BRow class="mb-3 d-flex align-items-center">
        <BCol md="2">
          <strong>Owner:</strong>
        </BCol>
        <BCol md="4">
          <div class="d-flex align-items-center">
            <span
              v-b-tooltip.hover
              :title="endpoint.owner_id"
            >{{ endpoint.owner_name }}</span>
            <BButton
              size="sm"
              variant="link"
              @click="copyToClipboard(endpoint.owner_id, 'ownerId')"
            >
              <i class="bi bi-clipboard" />
            </BButton>
            <span v-if="copied === 'ownerId'">Copied!</span>
          </div>
        </BCol>
        <BCol md="2">
          <strong>Public:</strong>
        </BCol>
        <BCol md="4">
          {{ endpoint.is_public ? 'Yes' : 'No' }}
        </BCol>
      </BRow>
      <BRow class="mb-3 d-flex align-items-center">
        <BCol md="2">
          <strong>Created At:</strong>
        </BCol>
        <BCol md="4">
          {{ new Date(endpoint.created_at).toLocaleString() }}
        </BCol>
        <BCol md="2">
          <strong>Updated At:</strong>
        </BCol>
        <BCol md="4">
          {{ new Date(endpoint.updated_at).toLocaleString() }}
        </BCol>
      </BRow>
      <BRow class="mb-3 d-flex align-items-center">
        <BCol md="2">
          <strong>Description:</strong>
        </BCol>
        <BCol md="4">
          {{ endpoint.description }}
        </BCol>
        <BCol md="2">
          <strong>Options:</strong>
        </BCol>
        <BCol md="4">
          {{ endpoint.options }}
        </BCol>
      </BRow>
    </div>

    <!-- Delete Endpoint Modal -->
    <BModal
      v-model="isDeleteModalVisible"
      title="Confirm Delete Endpoint"
      @ok="handleDeleteEndpoint"
      ok-title="Delete"
      @hidden="resetDeleteModal"
      ok-variant="danger"
    >
      <p>Are you sure you want to delete endpoint <strong>{{ endpoint.name }}</strong> (ID: {{ endpoint.id }})?</p>
      <p class="text-danger">
        This action cannot be undone.
      </p>
    </BModal>
  </BCard>
</template>

<script setup>
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { BCard, BRow, BCol, vBTooltip, BButton, BModal } from 'bootstrap-vue-next'
import { TransportServiceType } from '../../service/enum'
import apiClient from '../../service/api'
import { useToast } from 'vue-toast-notification'

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

const emit = defineEmits(['endpoint-loaded'])
const router = useRouter()
const $toast = useToast()

const endpoint = ref({})
const copied = ref(null)

const copyToClipboard = (text, type) => {
  navigator.clipboard.writeText(text).then(() => {
    copied.value = type
    setTimeout(() => {
      copied.value = null
    }, 2000)
  })
}

async function fetchEndpoint() {
  try {
    const url = props.isAdmin ? `/v1/admin/endpoint/${props.endpointId}` : `/v1/endpoint/${props.endpointId}`
    const epRes = await apiClient.get(url)
    endpoint.value = epRes.data.data
    emit('endpoint-loaded', endpoint.value)
  } catch (error) {
    console.error('Failed to fetch endpoint data:', error)
    $toast.error('Failed to fetch endpoint data: ' + (error.response?.data?.message || error.message))
  }
}

const isExpanded = ref(false)
const toggleExpand = () => {
  isExpanded.value = !isExpanded.value
}

// Delete Endpoint Modal
const isDeleteModalVisible = ref(false)
const showDeleteModal = () => {
  isDeleteModalVisible.value = true
}

const handleDeleteEndpoint = async () => {
  try {
    await apiClient.delete(`/v1/endpoint/${props.endpointId}`)
    $toast.success(`Endpoint '${endpoint.value.name}' deleted successfully.`)
    router.push({ name: 'Home' })
  } catch (error) {
    console.error('Failed to delete endpoint:', error)
    $toast.error('Failed to delete endpoint: ' + (error.response?.data?.message || error.message))
  }
}

const resetDeleteModal = () => {
  isDeleteModalVisible.value = false
}

onMounted(() => {
  fetchEndpoint()
})
</script>

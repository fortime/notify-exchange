<template>
  <BCard>
    <template #header>
      <div class="d-flex justify-content-between align-items-center">
        <span>Pending Users</span>
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
      v-if="pendingUsers && pendingUsers.length > 0"
      :items="pendingUsers"
      :fields="fields"
      striped
      responsive
    >
      <template #cell(id)="{ item }">
        <span>{{ item.id }}</span>
        <BButton
          size="sm"
          variant="link"
          @click="copyToClipboard(item.id, `pending-user-id-${item.id}`)"
        >
          <i class="bi bi-clipboard" />
        </BButton>
        <span v-if="copied === `pending-user-id-${item.id}`">Copied!</span>
      </template>
      <template #cell(email)="{ item }">
        <span>{{ item.email }}</span>
        <BButton
          size="sm"
          variant="link"
          @click="copyToClipboard(item.email, `pending-user-email-${item.id}`)"
        >
          <i class="bi bi-clipboard" />
        </BButton>
        <span v-if="copied === `pending-user-email-${item.id}`">Copied!</span>
      </template>
      <template #cell(username)="{ item }">
        <span>{{ item.username }}</span>
      </template>
      <template #cell(created_at)="{ item }">
        {{ new Date(item.created_at).toLocaleString() }}
      </template>
      <template #cell(expired_at)="{ item }">
        {{ new Date(item.expired_at).toLocaleString() }}
      </template>
      <template #cell(actions)="{ item }">
        <div class="d-flex align-items-center">
          <BButton
            class="mx-1"
            size="sm"
            variant="success"
            @click="showConfirmModal(item)"
          >
            Confirm
          </BButton>
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
      No pending user found.
    </div>

    <hr>

    <AppPagination
      url="/v1/admin/pending-user"
      data-key="pending_users"
      @fetched="handleFetched"
      ref="paginationRef"
    />

    <!-- Confirm User Modal -->
    <BModal
      v-model="confirmModalShown"
      title="Confirm Pending User"
      @ok="confirmPendingUser"
    >
      <p>Are you sure you want to confirm pending user <strong>{{ selectedPendingUser ? selectedPendingUser.username : '' }}</strong> ({{ selectedPendingUser ? selectedPendingUser.email : '' }})?</p>
    </BModal>

    <!-- Delete User Modal -->
    <BModal
      v-model="deleteModalShown"
      title="Delete Pending User"
      ok-variant="danger"
      ok-title="Delete"
      @ok="deletePendingUser"
    >
      <p>Are you sure you want to delete pending user <strong>{{ selectedPendingUser ? selectedPendingUser.username : '' }}</strong> ({{ selectedPendingUser ? selectedPendingUser.email : '' }})?</p>
    </BModal>
  </BCard>
</template>

<script setup>
import { ref } from 'vue'
import { BCard, BTable, BButton, BModal } from 'bootstrap-vue-next'
import apiClient from '../../service/api'
import { useToast } from 'vue-toast-notification'
import AppPagination from '../AppPagination.vue'

const $toast = useToast()
const paginationRef = ref(null)
const pendingUsers = ref([])

const fields = [
  { key: 'id', label: 'ID', tdClass: 'align-middle' },
  { key: 'username', label: 'Username', tdClass: 'align-middle' },
  { key: 'email', label: 'Email', tdClass: 'align-middle' },
  { key: 'created_at', label: 'Created At', tdClass: 'align-middle' },
  { key: 'expired_at', label: 'Expired At', tdClass: 'align-middle' },
  { key: 'actions', label: 'Actions', tdClass: 'align-middle' },
]

const handleFetched = (items) => {
  pendingUsers.value = items
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

const confirmModalShown = ref(false)
const deleteModalShown = ref(false)
const selectedPendingUser = ref(null)

const showConfirmModal = (user) => {
  selectedPendingUser.value = user
  confirmModalShown.value = true
}

const showDeleteModal = (user) => {
  selectedPendingUser.value = user
  deleteModalShown.value = true
}

const confirmPendingUser = async () => {
  try {
    await apiClient.put(`/v1/admin/pending-user/${selectedPendingUser.value.id}`)
    $toast.success('Pending user confirmed successfully.')
    paginationRef.value?.fetchData()
  } catch (error) {
    console.error('Failed to confirm pending user:', error)
    $toast.error('Failed to confirm pending user: ' + (error.response?.data?.message || error.message))
  }
  confirmModalShown.value = false
}

const deletePendingUser = async () => {
  try {
    await apiClient.delete(`/v1/admin/pending-user/${selectedPendingUser.value.id}`)
    $toast.success('Pending user deleted successfully.')
    paginationRef.value?.fetchData()
  } catch (error) {
    console.error('Failed to delete pending user:', error)
    $toast.error('Failed to delete pending user: ' + (error.response?.data?.message || error.message))
  }
  deleteModalShown.value = false
}
</script>

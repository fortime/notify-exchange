<template>
  <BCard>
    <template #header>
      <div class="d-flex justify-content-between align-items-center">
        <span>Http Endpoint Secrets</span>
        <div class="d-flex gap-2">
          <BButton
            size="sm"
            variant="primary"
            :disabled="authPubKeyCount >= 3"
            @click="showAddPubKeyModal"
          >
            Add Auth Public Key
          </BButton>
          <BButton
            size="sm"
            variant="primary"
            :disabled="encryptSecretCount >= 3"
            @click="showAddEncryptSecretModal"
          >
            Add Encrypt Secret
          </BButton>
          <BButton
            size="sm"
            variant="outline-primary"
            @click="paginationRef?.fetchData()"
          >
            Refresh
          </BButton>
        </div>
      </div>
    </template>
    <BTable
      v-if="secrets && secrets.length > 0"
      :items="secrets"
      :fields="fields"
      striped
      hover
      responsive
    >
      <template #cell(secret_type)="{ item }">
        {{ getSecretTypeDisplayName(item.secret_type) }}
      </template>
      <template #cell(updated_at)="{ item }">
        {{ new Date(item.updated_at).toLocaleString() }}
      </template>
      <template #cell(actions)="{ item }">
        <div class="d-flex align-items-center">
          <BButton
            class="mx-1"
            size="sm"
            variant="success"
            @click="showViewSecretModal(item)"
          >
            View
          </BButton>
          <BButton
            v-if="item.secret_type === 'auth-pub-key'"
            class="mx-1"
            size="sm"
            variant="info"
            @click="showUpdatePubKeyModal(item)"
          >
            Update&nbsp;
          </BButton>
          <BButton
            v-if="item.secret_type === 'encrypt-secret'"
            class="mx-1"
            size="sm"
            variant="info"
            @click="showRefreshModal(item)"
          >
            Refresh
          </BButton>
          <BButton
            class="mx-1"
            size="sm"
            variant="danger"
            @click="showDeleteModal(item)"
          >
            Delete
          </BButton>
          <BButton
            v-if="item.revertable"
            class="mx-1"
            size="sm"
            variant="warning"
          >
            Revert
          </BButton>
        </div>
      </template>
    </BTable>
    <div
      v-else
      class="text-center text-muted"
    >
      No secrets have been added yet.
    </div>

    <AppPagination
      :url="`/v1/endpoint/${props.endpointId}/secret`"
      data-key="secrets"
      @fetched="handleFetched"
      ref="paginationRef"
    />

    <!-- Add Encrypt Secret Modal -->
    <BModal
      v-model="isAddEncryptSecretModalVisible"
      title="Add Encrypt Secret"
      @ok="handleAddEncryptSecret"
      :ok-disabled="!isNewEncryptSecretCodeValid"
      ok-title="Add"
      @hidden="resetEncryptSecretModal"
    >
      <BForm @submit.prevent="handleAddEncryptSecret">
        <BFormGroup
          label="Secret Code:"
          label-for="encrypt-secret-code-input"
          description="Code must be 4-64 characters, containing a-z, A-Z, 0-9, -, _."
          invalid-feedback="Secret Code must be 4-64 characters and contain only a-z, A-Z, 0-9, -, _."
          :state="isNewEncryptSecretCodeValid"
        >
          <BFormInput
            id="encrypt-secret-code-input"
            v-model="newEncryptSecretCode"
            :state="isNewEncryptSecretCodeValid"
            minlength="4"
            maxlength="64"
            required
          />
        </BFormGroup>
      </BForm>
    </BModal>

    <!-- Add Auth Public Key Modal -->
    <BModal
      v-model="isAddPubKeyModalVisible"
      title="Add Auth Public Key"
      @ok="handleAddPubKey"
      :ok-disabled="!isNewPubKeyCodeValid || !isPubKeyFileValid"
      ok-title="Add"
      @hidden="resetAddPubKeyModal"
    >
      <BForm @submit.prevent="handleAddPubKey">
        <BFormGroup
          label="Key Code:"
          label-for="pubkey-code-input"
          description="Code must be 4-64 characters, containing a-z, A-Z, 0-9, -, _."
          invalid-feedback="Key Code must be 4-64 characters and contain only a-z, A-Z, 0-9, -, _."
          :state="isNewPubKeyCodeValid"
        >
          <BFormInput
            id="pubkey-code-input"
            v-model="newPubKeyCode"
            :state="isNewPubKeyCodeValid"
            minlength="4"
            maxlength="64"
            required
          />
        </BFormGroup>
        <BFormGroup
          label="PEM File:"
          label-for="pubkey-file-input"
          invalid-feedback="Please select a PEM file."
          :state="isPubKeyFileValid"
        >
          <BFormFile
            id="pubkey-file-input"
            v-model="newPubKeyFile"
            :state="isPubKeyFileValid"
            accept=".pem"
            required
          />
        </BFormGroup>
      </BForm>
    </BModal>

    <!-- Update Auth Public Key Modal -->
    <BModal
      v-model="isUpdatePubKeyModalVisible"
      title="Update Auth Public Key"
      @ok="handleUpdatePubKey"
      :ok-disabled="!isPubKeyFileForUpdateValid"
      ok-title="Update"
      @hidden="resetUpdatePubKeyModal"
    >
      <p>Updating public key for code: <strong>{{ secretToUpdate?.code }}</strong></p>
      <BForm @submit.prevent="handleUpdatePubKey">
        <BFormGroup
          label="New PEM File:"
          label-for="update-pubkey-file-input"
          invalid-feedback="Please select a PEM file."
          :state="isPubKeyFileForUpdateValid"
        >
          <BFormFile
            id="update-pubkey-file-input"
            v-model="newPubKeyFileForUpdate"
            :state="isPubKeyFileForUpdateValid"
            accept=".pem"
            required
          />
        </BFormGroup>
      </BForm>
    </BModal>

    <!-- Refresh Secret Modal -->
    <BModal
      v-model="isRefreshModalVisible"
      title="Confirm Refresh"
      @ok="handleRefreshSecret"
      ok-title="Confirm"
      @hidden="resetRefreshModal"
    >
      <p>Are you sure you want to refresh the secret with code: <strong>{{ secretToRefresh?.code }}</strong>?</p>
    </BModal>

    <!-- View Secret Modal -->
    <BModal
      v-model="isViewSecretModalVisible"
      :title="`Secret: ${secretToView?.code}`"
      ok-only
      ok-title="Close"
      @hidden="resetViewSecretModal"
    >
      <div v-if="secretToView?.content">
        <strong>Content:</strong>
        <pre>{{ secretToView.content }}</pre>
      </div>
      <div v-else>
        Unable to show the content of this secret
      </div>
    </BModal>

    <!-- Generated Secret Modal -->
    <BModal
      v-model="isGeneratedSecretModalVisible"
      title="Generated Encrypt Secret"
      ok-only
      ok-title="Close"
      @hidden="resetGeneratedSecretModal"
    >
      <p class="text-danger">
        This secret will only be shown once. Please copy it and store it somewhere safe.
      </p>
      <BAlert
        show
        variant="success"
        body-class="d-flex align-items-center container-lg"
      >
        <pre class="me-2 mb-0">{{ generatedSecret }}</pre>
        <div class="ms-auto">
          <BButton
            @click="copyToClipboard(generatedSecret, 'generated')"
            size="sm"
            variant="outline-secondary"
          >
            <i class="bi bi-clipboard" />
          </BButton>
          <span v-if="copied === 'generated'">Copied!</span>
        </div>
      </BAlert>
    </BModal>

    <!-- Delete Secret Modal -->
    <BModal
      v-model="isDeleteModalVisible"
      title="Confirm Delete"
      @ok="handleDeleteSecret"
      ok-title="Confirm"
      @hidden="resetDeleteModal"
    >
      <p>Are you sure you want to delete the secret with code: <strong>{{ secretToDelete?.code }}</strong>?</p>
    </BModal>
  </BCard>
</template>

<script setup>
import { ref, computed } from 'vue'
import {
  BAlert,
  BCard,
  BTable,
  BButton,
  BModal,
  BForm,
  BFormGroup,
  BFormInput,
  BFormFile
} from 'bootstrap-vue-next'
import { useToast } from 'vue-toast-notification'

import apiClient from '../../../service/api'
import AppPagination from '../../AppPagination.vue'

const $toast = useToast()
const paginationRef = ref(null)
const secrets = ref([])

const props = defineProps({
  endpointId: {
    type: String,
    required: true
  }
})

const fields = [
  { key: 'code', label: 'Code', tdClass: 'align-middle' },
  { key: 'secret_type', label: 'Type', tdClass: 'align-middle' },
  { key: 'updated_at', label: 'Updated At', tdClass: 'align-middle' },
  { key: 'actions', label: 'Actions', tdClass: 'align-middle' }
]

const handleFetched = (items) => {
  secrets.value = items
}

const getSecretTypeDisplayName = (type) => {
  switch (type) {
    case 'auth-pub-key':
      return 'Auth Public Key'
    case 'encrypt-secret':
      return 'Encrypt Secret'
    default:
      return type
  }
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

const authPubKeyCount = computed(() => {
  return secrets.value.filter(s => s.secret_type === 'auth-pub-key').length
})

const encryptSecretCount = computed(() => {
  return secrets.value.filter(s => s.secret_type === 'encrypt-secret').length
})

// Add Auth Public Key Modal
const isAddPubKeyModalVisible = ref(false)
const newPubKeyCode = ref('')
const newPubKeyFile = ref(null)

const isNewPubKeyCodeValid = computed(() => {
  const code = newPubKeyCode.value
  if (!code) return null
  return /^[a-zA-Z0-9-_]{4,64}$/.test(code) && code.length >= 4 && code.length <= 64
})

const isPubKeyFileValid = computed(() => {
    return newPubKeyFile.value && newPubKeyFile.value.name && newPubKeyFile.value.name.endsWith(".pem")
})

const showAddPubKeyModal = () => {
  resetAddPubKeyModal()
  isAddPubKeyModalVisible.value = true
}

const handleAddPubKey = async () => {
  if (isNewPubKeyCodeValid.value && isPubKeyFileValid.value) {
    try {
      const formData = new FormData()
      formData.append('code', newPubKeyCode.value)
      formData.append('secret_type', 'auth-pub-key')
      formData.append('secret', newPubKeyFile.value)
      await apiClient.post(`/v1/endpoint/${props.endpointId}/secret`, formData, {
        headers: { 'Content-Type': 'multipart/form-data' }
      })
      paginationRef.value?.fetchData()
      isAddPubKeyModalVisible.value = false
    } catch (error) {
      console.error('Failed to add auth public key:', error)
      $toast.error('Failed to add auth public key: ' + (error.response?.data?.message || error.message))
    }
  }
}

const resetAddPubKeyModal = () => {
  newPubKeyCode.value = ''
  newPubKeyFile.value = null
}

// Add Encrypt Secret Modal
const isAddEncryptSecretModalVisible = ref(false)
const newEncryptSecretCode = ref('')

const isNewEncryptSecretCodeValid = computed(() => {
  const code = newEncryptSecretCode.value
  if (!code) return null // Initial state, no validation yet
  return /^[a-zA-Z0-9-_]{4,64}$/.test(code) && code.length >= 4 && code.length <= 64
})

const showAddEncryptSecretModal = () => {
  resetEncryptSecretModal()
  isAddEncryptSecretModalVisible.value = true
}

const handleAddEncryptSecret = async () => {
  if (isNewEncryptSecretCodeValid.value) {
    try {
      const formData = new FormData()
      formData.append('code', newEncryptSecretCode.value)
      formData.append('secret_type', 'encrypt-secret')
      const res = await apiClient.post(`/v1/endpoint/${props.endpointId}/secret`, formData, {
        headers: { 'Content-Type': 'multipart/form-data' }
      })
      generatedSecret.value = res.data.data.base64_secret
      isAddEncryptSecretModalVisible.value = false
      isGeneratedSecretModalVisible.value = true
    } catch (error) {
      console.error('Failed to add encrypt secret:', error)
      $toast.error('Failed to add encrypt secret: ' + (error.response?.data?.message || error.message))
    }
  }
}

const resetEncryptSecretModal = () => {
  newEncryptSecretCode.value = ''
}

const isGeneratedSecretModalVisible = ref(false)
const generatedSecret = ref('')

const resetGeneratedSecretModal = () => {
  generatedSecret.value = ''
  paginationRef.value?.fetchData()
}

// Refresh Secret Modal
const isRefreshModalVisible = ref(false)
const secretToRefresh = ref(null)

const showRefreshModal = (secret) => {
  secretToRefresh.value = { ...secret } // Create a shallow copy
  isRefreshModalVisible.value = true
}

const handleRefreshSecret = async () => {
  const secret = secretToRefresh.value
  if (secret) {
    try {
      const params = new URLSearchParams({
        code: secret.code,
        secret_type: secret.secret_type
      }).toString()
      const res = await apiClient.put(`/v1/endpoint/${props.endpointId}/secret?${params}`, {}, {
        headers: { 'Content-Type': 'multipart/form-data' }
      })
      generatedSecret.value = res.data.data.base64_secret
      isRefreshModalVisible.value = false
      isGeneratedSecretModalVisible.value = true
      $toast.success('Successfully refreshed secret.')
    } catch (error) {
      console.error('Failed to refresh secret:', error)
      $toast.error('Failed to refresh secret: ' + (error.response?.data?.message || error.message))
    }
  }
}

const resetRefreshModal = () => {
  secretToRefresh.value = null
}

// Update Public Key Modal
const isUpdatePubKeyModalVisible = ref(false)
const secretToUpdate = ref(null)
const newPubKeyFileForUpdate = ref(null)

const isPubKeyFileForUpdateValid = computed(() => {
    return newPubKeyFileForUpdate.value && newPubKeyFileForUpdate.value.name && newPubKeyFileForUpdate.value.name.endsWith(".pem")
})

const showUpdatePubKeyModal = (secret) => {
  secretToUpdate.value = { ...secret } // Create a shallow copy
  newPubKeyFileForUpdate.value = null
  isUpdatePubKeyModalVisible.value = true
}

const handleUpdatePubKey = async () => {
  const secret = secretToUpdate.value
  if (secret && isPubKeyFileForUpdateValid.value) {
    try {
      const formData = new FormData()
      formData.append('secret', newPubKeyFileForUpdate.value)
      const params = new URLSearchParams({
        code: secret.code,
        secret_type: secret.secret_type
      }).toString()
      await apiClient.put(`/v1/endpoint/${props.endpointId}/secret?${params}`, formData, {
        headers: { 'Content-Type': 'multipart/form-data' }
      })
      paginationRef.value?.fetchData()
      isUpdatePubKeyModalVisible.value = false
      $toast.success('Successfully updated public key.')
    } catch (error) {
      console.error('Failed to update public key:', error)
      $toast.error('Failed to update public key: ' + (error.response?.data?.message || error.message))
    }
  }
}

const resetUpdatePubKeyModal = () => {
  secretToUpdate.value = null
  newPubKeyFileForUpdate.value = null
}

// View Secret Modal
const isViewSecretModalVisible = ref(false)
const secretToView = ref(null)

const showViewSecretModal = async (secret) => {
  secretToView.value = { ...secret, content: 'Loading...' } // Create a shallow copy
  isViewSecretModalVisible.value = true
  try {
    const res = await apiClient.get(`/v1/endpoint/${props.endpointId}/secret/${secret.id}/masked`)
    secretToView.value.content = res.data.data.masked_secret
  } catch (error) {
    console.error('Failed to fetch masked secret:', error)
    $toast.error('Failed to fetch masked secret: ' + (error.response?.data?.message || error.message))
    secretToView.value.content = 'Failed to load secret.'
  }
}

const resetViewSecretModal = () => {
  secretToView.value = null
}

// Delete Secret Modal
const isDeleteModalVisible = ref(false)
const secretToDelete = ref(null)

const showDeleteModal = (secret) => {
  secretToDelete.value = { ...secret } // Create a shallow copy
  isDeleteModalVisible.value = true
}

const handleDeleteSecret = async () => {
  const secret = secretToDelete.value
  if (secret) {
    try {
      const params = new URLSearchParams({
        code: secret.code,
        secret_type: secret.secret_type
      }).toString()
      await apiClient.delete(`/v1/endpoint/${props.endpointId}/secret?${params}`)
      paginationRef.value?.fetchData()
      isDeleteModalVisible.value = false
      $toast.success('Successfully deleted secret.')
    } catch (error) {
      console.error('Failed to delete secret:', error)
      $toast.error('Failed to delete secret: ' + (error.response?.data?.message || error.message))
    }
  }
}

const resetDeleteModal = () => {
  secretToDelete.value = null
}
</script>

<style scoped>
pre {
  white-space: pre-wrap;
  word-wrap: break-word;
}
</style>

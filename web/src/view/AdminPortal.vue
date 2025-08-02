<template>
  <div class="mt-5">
    <BRow>
      <BCol>
        <PendingUsersCard class="mb-4" />
      </BCol>
    </BRow>
    <BRow>
      <BCol>
        <UsersCard class="mb-4" />
      </BCol>
    </BRow>
    <BRow>
      <BCol>
        <TopicsCard class="mb-4" />
      </BCol>
    </BRow>
    <BRow>
      <BCol>
        <TransportServicesCard class="mb-4" />
      </BCol>
    </BRow>
  </div>
</template>

<script setup>
import { BRow, BCol } from 'bootstrap-vue-next'
import PendingUsersCard from '../component/admin/PendingUsersCard.vue'
import UsersCard from '../component/admin/UsersCard.vue'
import TopicsCard from '../component/admin/TopicsCard.vue'
import TransportServicesCard from '../component/admin/TransportServicesCard.vue'
</script>

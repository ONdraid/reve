<template>
  <div class="upscale-options">
    <v-select
      :disabled="props.disabled"
      label="Upscale Factor"
      v-model="selectType"
      variant="solo"
      :items="[
        {
          text: '2x',
          value: '2',
        },
        {
          text: '3x',
          value: '3',
        },
        {
          text: '4x',
          value: '4',
        },
      ]"
      item-title="text"
      item-value="value"
    ></v-select>
  </div>
</template>

<script setup lang="ts">
import { ref, watch, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/tauri";

const props = defineProps<{
  disabled: boolean;
}>();

const emit = defineEmits(["upscale-factor-changed"]);

// The upscale type. Default is `general`.
const selectType = ref("2x");

function updateUpscaleFactor(value: any) {
  emit("upscale-factor-changed", value.target.value);
}
</script>

<style scoped lang="scss">
.upscale-options {
  text-align: left;
  align-items: flex-start;
}
</style>

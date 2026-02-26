import { gql } from "@apollo/client";

export const SEND_OTP = gql`
  mutation SendOtp($phone: String!) {
    sendOtp(phone: $phone) {
      success
    }
  }
`;

export const VERIFY_OTP = gql`
  mutation VerifyOtp($phone: String!, $code: String!) {
    verifyOtp(phone: $phone, code: $code) {
      success
    }
  }
`;

export const LOGOUT = gql`
  mutation Logout {
    logout {
      success
    }
  }
`;

export const ADD_SOURCE = gql`
  mutation AddSource($url: String!, $reason: String) {
    addSource(url: $url, reason: $reason) {
      success
      sourceId
    }
  }
`;

export const RUN_SCOUT = gql`
  mutation RunScout($taskId: String!) {
    runScout(taskId: $taskId) {
      success
      message
    }
  }
`;

export const STOP_SCOUT = gql`
  mutation StopScout($taskId: String!) {
    stopScout(taskId: $taskId) {
      success
      message
    }
  }
`;

export const RESET_SCOUT_STATUS = gql`
  mutation ResetScoutStatus($taskId: String!) {
    resetScoutStatus(taskId: $taskId) {
      success
      message
    }
  }
`;

export const RUN_SCOUT_PHASE = gql`
  mutation RunScoutPhase($phase: ScoutPhase!, $taskId: String!) {
    runScoutPhase(phase: $phase, taskId: $taskId) {
      success
      message
    }
  }
`;

export const CREATE_SCOUT_TASK = gql`
  mutation CreateScoutTask(
    $location: String!
    $radiusKm: Float
    $priority: Float
  ) {
    createScoutTask(
      location: $location
      radiusKm: $radiusKm
      priority: $priority
    )
  }
`;

export const CANCEL_SCOUT_TASK = gql`
  mutation CancelScoutTask($id: String!) {
    cancelScoutTask(id: $id)
  }
`;

export const MERGE_TAGS = gql`
  mutation MergeTags($sourceSlug: String!, $targetSlug: String!) {
    mergeTags(sourceSlug: $sourceSlug, targetSlug: $targetSlug) {
      success
    }
  }
`;

export const UPDATE_SOURCE = gql`
  mutation UpdateSource(
    $id: UUID!
    $active: Boolean
    $weight: Float
    $qualityPenalty: Float
  ) {
    updateSource(
      id: $id
      active: $active
      weight: $weight
      qualityPenalty: $qualityPenalty
    ) {
      success
    }
  }
`;

export const DELETE_SOURCE = gql`
  mutation DeleteSource($id: UUID!) {
    deleteSource(id: $id) {
      success
    }
  }
`;

export const SUBMIT_SOURCE = gql`
  mutation SubmitSource($url: String!) {
    submitSource(url: $url) {
      success
      sourceId
    }
  }
`;

export const DISMISS_FINDING = gql`
  mutation DismissFinding($id: String!) {
    dismissFinding(id: $id)
  }
`;

export const SCRAPE_URL = gql`
  mutation ScrapeUrl($url: String!) {
    scrapeUrl(url: $url) {
      success
      message
    }
  }
`;

export const PURGE_AREA = gql`
  mutation PurgeArea($taskId: String!) {
    purgeArea(taskId: $taskId) {
      success
      message
      signals
      situations
    }
  }
`;

export const RE_EXTRACT_SIGNAL = gql`
  mutation ReExtractSignal($signalId: UUID!) {
    reExtractSignal(signalId: $signalId) {
      sourceUrl
      signals {
        signalType
        title
        summary
        sensitivity
        latitude
        longitude
        locationName
        startsAt
        endsAt
        organizer
        urgency
        severity
        category
        contentDate
        tags
        isFirsthand
        whatWouldHelp
      }
      rejected {
        title
        sourceUrl
        reason
      }
    }
  }
`;

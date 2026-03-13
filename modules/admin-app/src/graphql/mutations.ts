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

export const RUN_BOOTSTRAP = gql`
  mutation RunBootstrap($regionId: String!) {
    runBootstrap(regionId: $regionId) {
      success
      message
    }
  }
`;

export const RUN_SCRAPE = gql`
  mutation RunScrape($regionId: String!) {
    runScrape(regionId: $regionId) {
      success
      message
    }
  }
`;

export const RUN_WEAVE = gql`
  mutation RunWeave($regionId: String!) {
    runWeave(regionId: $regionId) {
      success
      message
    }
  }
`;

export const RUN_SCOUT_SOURCE = gql`
  mutation RunScoutSource($sourceIds: [String!]!) {
    runScoutSource(sourceIds: $sourceIds) {
      success
      message
    }
  }
`;

export const CANCEL_RUN = gql`
  mutation CancelRun($runId: String!) {
    cancelRun(runId: $runId) {
      success
      message
    }
  }
`;

export const CREATE_SCHEDULE = gql`
  mutation CreateSchedule($flowType: String!, $scope: String!, $cadenceSeconds: Int!, $regionId: String) {
    createSchedule(flowType: $flowType, scope: $scope, cadenceSeconds: $cadenceSeconds, regionId: $regionId) {
      success
      message
    }
  }
`;

export const TOGGLE_SCHEDULE = gql`
  mutation ToggleSchedule($scheduleId: String!, $enabled: Boolean!) {
    toggleSchedule(scheduleId: $scheduleId, enabled: $enabled) {
      success
      message
    }
  }
`;

export const DELETE_SCHEDULE = gql`
  mutation DeleteSchedule($scheduleId: String!) {
    deleteSchedule(scheduleId: $scheduleId) {
      success
      message
    }
  }
`;

export const UPDATE_SCHEDULE_CADENCE = gql`
  mutation UpdateScheduleCadence($scheduleId: String!, $cadenceSeconds: Int!) {
    updateScheduleCadence(scheduleId: $scheduleId, cadenceSeconds: $cadenceSeconds) {
      success
      message
    }
  }
`;

export const CREATE_REGION = gql`
  mutation CreateRegion($name: String!) {
    createRegion(name: $name) {
      id
      name
      centerLat
      centerLng
      radiusKm
      geoTerms
      isLeaf
      createdAt
    }
  }
`;

export const DELETE_REGION = gql`
  mutation DeleteRegion($id: String!) {
    deleteRegion(id: $id)
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
    $channelWeights: [ChannelWeightInput!]
  ) {
    updateSource(
      id: $id
      active: $active
      weight: $weight
      qualityPenalty: $qualityPenalty
      channelWeights: $channelWeights
    ) {
      success
    }
  }
`;

export const CLEAR_SOURCE_SIGNALS = gql`
  mutation ClearSourceSignals($sourceId: UUID!) {
    clearSourceSignals(sourceId: $sourceId) {
      success
      message
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

export const DELETE_ACTOR = gql`
  mutation DeleteActor($id: UUID!) {
    deleteActor(id: $id) {
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

export const SET_BUDGET = gql`
  mutation SetBudget($dailyLimitCents: Int!, $perRunMaxCents: Int!) {
    setBudget(dailyLimitCents: $dailyLimitCents, perRunMaxCents: $perRunMaxCents)
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


export const WEAVE_CLUSTER = gql`
  mutation WeaveCluster($groupId: String!) {
    weaveCluster(groupId: $groupId) {
      success
      message
    }
  }
`;

export const FEED_GROUP = gql`
  mutation FeedGroup($groupId: String!) {
    feedGroup(groupId: $groupId) {
      success
      message
    }
  }
`;

export const COALESCE_SIGNAL = gql`
  mutation CoalesceSignal($signalId: String!) {
    coalesceSignal(signalId: $signalId) {
      success
      message
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
        opposing
      }
      rejected {
        title
        sourceUrl
        reason
      }
    }
  }
`;

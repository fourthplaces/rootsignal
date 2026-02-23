import { gql } from "@apollo/client";

// Shared fields across all signal types (from signal_meta_resolvers macro)
const SIGNAL_FIELDS = `
  ... on GqlGatheringSignal {
    id title summary sensitivity confidence location { lat lng precision }
    locationName sourceUrl extractedAt sourceDiversity causeHeat channelDiversity
    startsAt endsAt actionUrl organizer isRecurring
  }
  ... on GqlAidSignal {
    id title summary sensitivity confidence location { lat lng precision }
    locationName sourceUrl extractedAt sourceDiversity causeHeat channelDiversity
    actionUrl availability isOngoing
  }
  ... on GqlNeedSignal {
    id title summary sensitivity confidence location { lat lng precision }
    locationName sourceUrl extractedAt sourceDiversity causeHeat channelDiversity
    urgency whatNeeded actionUrl goal
  }
  ... on GqlNoticeSignal {
    id title summary sensitivity confidence location { lat lng precision }
    locationName sourceUrl extractedAt sourceDiversity causeHeat channelDiversity
    severity category effectiveDate sourceAuthority
  }
  ... on GqlTensionSignal {
    id title summary sensitivity confidence location { lat lng precision }
    locationName sourceUrl extractedAt sourceDiversity causeHeat channelDiversity
    severity category whatWouldHelp
  }
`;

export const ME = gql`
  query Me {
    me {
      isAdmin
      phoneNumber
    }
  }
`;

export const ADMIN_DASHBOARD = gql`
  query AdminDashboard($region: String!) {
    adminDashboard(region: $region) {
      totalSignals
      totalStories
      totalActors
      totalSources
      activeSources
      totalTensions
      scoutStatuses {
        regionName
        regionSlug
        lastScouted
        sourcesDue
        running
        phaseStatus
      }
      signalVolumeByDay {
        day
        gatherings
        aids
        needs
        notices
        tensions
      }
      countByType {
        signalType
        count
      }
      storyCountByArc {
        label
        count
      }
      storyCountByCategory {
        label
        count
      }
      freshnessDistribution {
        label
        count
      }
      confidenceDistribution {
        label
        count
      }
      unmetTensions {
        title
        severity
        category
        whatWouldHelp
      }
      topSources {
        name
        signals
        weight
        emptyRuns
      }
      bottomSources {
        name
        signals
        weight
        emptyRuns
      }
      extractionYield {
        sourceLabel
        extracted
        survived
        corroborated
        contradicted
      }
      gapStats {
        gapType
        total
        successful
        avgWeight
      }
    }
  }
`;

export const ADMIN_REGION_SOURCES = gql`
  query AdminRegionSources($regionSlug: String!) {
    adminRegionSources(regionSlug: $regionSlug) {
      id
      url
      canonicalValue
      sourceLabel
      weight
      qualityPenalty
      effectiveWeight
      discoveryMethod
      lastScraped
      cadenceHours
      signalsProduced
      active
    }
  }
`;

export const ADMIN_SCOUT_TASKS = gql`
  query AdminScoutTasks($status: String, $limit: Int) {
    adminScoutTasks(status: $status, limit: $limit) {
      id
      centerLat
      centerLng
      radiusKm
      context
      geoTerms
      priority
      source
      status
      phaseStatus
      createdAt
      completedAt
    }
  }
`;

export const SIGNALS_NEAR = gql`
  query SignalsNear(
    $lat: Float!
    $lng: Float!
    $radiusKm: Float!
    $types: [SignalType!]
  ) {
    signalsNear(lat: $lat, lng: $lng, radiusKm: $radiusKm, types: $types) {
      ... on GqlGatheringSignal {
        id title summary sensitivity confidence location { lat lng precision }
        locationName sourceUrl extractedAt sourceDiversity causeHeat channelDiversity
        startsAt endsAt actionUrl organizer isRecurring
        actors { id name actorType }
      }
      ... on GqlAidSignal {
        id title summary sensitivity confidence location { lat lng precision }
        locationName sourceUrl extractedAt sourceDiversity causeHeat channelDiversity
        actionUrl availability isOngoing
        actors { id name actorType }
      }
      ... on GqlNeedSignal {
        id title summary sensitivity confidence location { lat lng precision }
        locationName sourceUrl extractedAt sourceDiversity causeHeat channelDiversity
        urgency whatNeeded actionUrl goal
        actors { id name actorType }
      }
      ... on GqlNoticeSignal {
        id title summary sensitivity confidence location { lat lng precision }
        locationName sourceUrl extractedAt sourceDiversity causeHeat channelDiversity
        severity category effectiveDate sourceAuthority
        actors { id name actorType }
      }
      ... on GqlTensionSignal {
        id title summary sensitivity confidence location { lat lng precision }
        locationName sourceUrl extractedAt sourceDiversity causeHeat channelDiversity
        severity category whatWouldHelp
        actors { id name actorType }
      }
    }
  }
`;

export const SIGNALS_NEAR_GEO_JSON = gql`
  query SignalsNearGeoJson(
    $lat: Float!
    $lng: Float!
    $radiusKm: Float!
    $types: [SignalType!]
  ) {
    signalsNearGeoJson(lat: $lat, lng: $lng, radiusKm: $radiusKm, types: $types)
  }
`;

export const SIGNALS_RECENT = gql`
  query SignalsRecent($limit: Int, $types: [SignalType!]) {
    signalsRecent(limit: $limit, types: $types) {
      ${SIGNAL_FIELDS}
    }
  }
`;

export const SIGNAL_DETAIL = gql`
  query Signal($id: UUID!) {
    signal(id: $id) {
      ${SIGNAL_FIELDS}
      ... on GqlGatheringSignal {
        evidence { id sourceUrl snippet relevance }
        actors { id name actorType }
        story { id headline arc }
      }
      ... on GqlAidSignal {
        evidence { id sourceUrl snippet relevance }
        actors { id name actorType }
        story { id headline arc }
      }
      ... on GqlNeedSignal {
        evidence { id sourceUrl snippet relevance }
        actors { id name actorType }
        story { id headline arc }
      }
      ... on GqlNoticeSignal {
        evidence { id sourceUrl snippet relevance }
        actors { id name actorType }
        story { id headline arc }
      }
      ... on GqlTensionSignal {
        evidence { id sourceUrl snippet relevance }
        actors { id name actorType }
        story { id headline arc }
      }
    }
  }
`;

export const STORIES = gql`
  query Stories($limit: Int, $status: String) {
    stories(limit: $limit, status: $status) {
      id
      headline
      summary
      arc
      category
      energy
      signalCount
      firstSeen
      status
    }
  }
`;

export const STORIES_IN_BOUNDS = gql`
  query StoriesInBounds($minLat: Float!, $maxLat: Float!, $minLng: Float!, $maxLng: Float!, $limit: Int) {
    storiesInBounds(minLat: $minLat, maxLat: $maxLat, minLng: $minLng, maxLng: $maxLng, limit: $limit) {
      id
      headline
      summary
      arc
      category
      energy
      signalCount
      firstSeen
      status
    }
  }
`;

export const STORY_DETAIL = gql`
  query Story($id: UUID!) {
    story(id: $id) {
      id
      headline
      summary
      arc
      category
      energy
      signalCount
      firstSeen
      lastUpdated
      velocity
      dominantType
      status
      lede
      narrative
      tags {
        slug
        name
      }
    }
  }
`;

export const ACTORS = gql`
  query Actors($region: String!, $limit: Int) {
    actors(region: $region, limit: $limit) {
      id
      name
      actorType
      description
      signalCount
    }
  }
`;

export const ALL_TAGS = gql`
  query Tags($limit: Int) {
    tags(limit: $limit) {
      slug
      name
    }
  }
`;

export const ADMIN_SCOUT_RUNS = gql`
  query AdminScoutRuns($region: String!, $limit: Int) {
    adminScoutRuns(region: $region, limit: $limit) {
      runId
      region
      startedAt
      finishedAt
      stats {
        urlsScraped
        urlsUnchanged
        urlsFailed
        signalsExtracted
        signalsDeduplicated
        signalsStored
        socialMediaPosts
        expansionQueriesCollected
        expansionSourcesCreated
      }
    }
  }
`;

export const ADMIN_SCOUT_RUN = gql`
  query AdminScoutRun($runId: String!) {
    adminScoutRun(runId: $runId) {
      runId
      region
      startedAt
      finishedAt
      stats {
        urlsScraped
        urlsUnchanged
        urlsFailed
        signalsExtracted
        signalsDeduplicated
        signalsStored
        socialMediaPosts
        expansionQueriesCollected
        expansionSourcesCreated
      }
      events {
        seq
        ts
        type
        query
        url
        provider
        platform
        identifier
        signalType
        title
        resultCount
        postCount
        items
        contentBytes
        contentChars
        signalsExtracted
        impliedQueries
        similarity
        confidence
        success
        action
        nodeId
        matchedId
        existingId
        sourceUrl
        newSourceUrl
        canonicalKey
        gatherings
        needs
        stale
        sourcesCreated
        spentCents
        remainingCents
        topics
        postsFound
      }
    }
  }
`;

export const SUPERVISOR_FINDINGS = gql`
  query SupervisorFindings($region: String!, $status: String, $limit: Int) {
    supervisorFindings(region: $region, status: $status, limit: $limit) {
      id
      issueType
      severity
      targetId
      targetLabel
      description
      suggestedAction
      status
      createdAt
      resolvedAt
    }
  }
`;

export const SUPERVISOR_SUMMARY = gql`
  query SupervisorSummary($region: String!) {
    supervisorSummary(region: $region) {
      totalOpen
      totalResolved
      totalDismissed
      countByType {
        label
        count
      }
      countBySeverity {
        label
        count
      }
    }
  }
`;

// --- Archive queries ---

export const ADMIN_ARCHIVE_COUNTS = gql`
  query AdminArchiveCounts {
    adminArchiveCounts {
      posts
      shortVideos
      stories
      longVideos
      pages
      feeds
      searchResults
      files
    }
  }
`;

export const ADMIN_ARCHIVE_VOLUME = gql`
  query AdminArchiveVolume($days: Int) {
    adminArchiveVolume(days: $days) {
      day
      posts
      shortVideos
      stories
      longVideos
      pages
      feeds
      searchResults
      files
    }
  }
`;

export const ADMIN_ARCHIVE_POSTS = gql`
  query AdminArchivePosts($limit: Int) {
    adminArchivePosts(limit: $limit) {
      id
      sourceUrl
      permalink
      author
      textPreview
      platform
      hashtags
      engagementSummary
      publishedAt
    }
  }
`;

export const ADMIN_ARCHIVE_SHORT_VIDEOS = gql`
  query AdminArchiveShortVideos($limit: Int) {
    adminArchiveShortVideos(limit: $limit) {
      id
      sourceUrl
      permalink
      textPreview
      engagementSummary
      publishedAt
    }
  }
`;

export const ADMIN_ARCHIVE_STORIES = gql`
  query AdminArchiveStories($limit: Int) {
    adminArchiveStories(limit: $limit) {
      id
      sourceUrl
      permalink
      textPreview
      location
      expiresAt
      fetchedAt
    }
  }
`;

export const ADMIN_ARCHIVE_LONG_VIDEOS = gql`
  query AdminArchiveLongVideos($limit: Int) {
    adminArchiveLongVideos(limit: $limit) {
      id
      sourceUrl
      permalink
      textPreview
      engagementSummary
      publishedAt
    }
  }
`;

export const ADMIN_ARCHIVE_PAGES = gql`
  query AdminArchivePages($limit: Int) {
    adminArchivePages(limit: $limit) {
      id
      sourceUrl
      title
      fetchedAt
    }
  }
`;

export const ADMIN_ARCHIVE_FEEDS = gql`
  query AdminArchiveFeeds($limit: Int) {
    adminArchiveFeeds(limit: $limit) {
      id
      sourceUrl
      title
      itemCount
      fetchedAt
    }
  }
`;

export const ADMIN_ARCHIVE_SEARCH_RESULTS = gql`
  query AdminArchiveSearchResults($limit: Int) {
    adminArchiveSearchResults(limit: $limit) {
      id
      query
      resultCount
      fetchedAt
    }
  }
`;

export const ADMIN_ARCHIVE_FILES = gql`
  query AdminArchiveFiles($limit: Int) {
    adminArchiveFiles(limit: $limit) {
      id
      url
      title
      mimeType
      duration
      pageCount
      fetchedAt
    }
  }
`;

// --- Situation queries ---

export const SITUATIONS_IN_BOUNDS = gql`
  query SituationsInBounds($minLat: Float!, $maxLat: Float!, $minLng: Float!, $maxLng: Float!, $limit: Int) {
    situationsInBounds(minLat: $minLat, maxLat: $maxLat, minLng: $minLng, maxLng: $maxLng, limit: $limit) {
      id
      headline
      arc
      temperature
      signalCount
      locationName
      firstSeen
      lastUpdated
    }
  }
`;

export const SITUATIONS = gql`
  query Situations($limit: Int) {
    situations(limit: $limit) {
      id
      headline
      lede
      arc
      temperature
      signalCount
      tensionCount
      dispatchCount
      centroidLat
      centroidLng
      locationName
      clarity
      firstSeen
      lastUpdated
    }
  }
`;

export const SITUATION_DETAIL = gql`
  query SituationDetail($id: UUID!) {
    situation(id: $id) {
      id
      headline
      lede
      arc
      temperature
      tensionHeat
      entityVelocity
      amplification
      responseCoverage
      clarityNeed
      clarity
      signalCount
      tensionCount
      dispatchCount
      centroidLat
      centroidLng
      locationName
      firstSeen
      lastUpdated
      sensitivity
      category
      dispatches(limit: 100) {
        id
        body
        signalIds
        createdAt
        dispatchType
        supersedes
        flaggedForReview
        flagReason
        fidelityScore
      }
    }
  }
`;

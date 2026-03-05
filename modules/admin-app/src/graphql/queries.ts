import { gql } from "@apollo/client";

// Shared fields across all signal types (from signal_meta_resolvers macro)
const SIGNAL_FIELDS = `
  ... on GqlGatheringSignal {
    id title summary sensitivity confidence location { lat lng precision }
    locationName sourceUrl extractedAt contentDate sourceDiversity causeHeat channelDiversity
    reviewStatus wasCorrected corrections rejectionReason
    startsAt endsAt actionUrl organizer isRecurring
  }
  ... on GqlResourceSignal {
    id title summary sensitivity confidence location { lat lng precision }
    locationName sourceUrl extractedAt contentDate sourceDiversity causeHeat channelDiversity
    reviewStatus wasCorrected corrections rejectionReason
    actionUrl availability isOngoing
  }
  ... on GqlHelpRequestSignal {
    id title summary sensitivity confidence location { lat lng precision }
    locationName sourceUrl extractedAt contentDate sourceDiversity causeHeat channelDiversity
    reviewStatus wasCorrected corrections rejectionReason
    urgency whatNeeded actionUrl statedGoal
  }
  ... on GqlAnnouncementSignal {
    id title summary sensitivity confidence location { lat lng precision }
    locationName sourceUrl extractedAt contentDate sourceDiversity causeHeat channelDiversity
    reviewStatus wasCorrected corrections rejectionReason
    severity subject effectiveDate sourceAuthority
  }
  ... on GqlConcernSignal {
    id title summary sensitivity confidence location { lat lng precision }
    locationName sourceUrl extractedAt contentDate sourceDiversity causeHeat channelDiversity
    reviewStatus wasCorrected corrections rejectionReason
    severity subject opposing
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
      totalActors
      totalSources
      activeSources
      totalConcerns
      scoutStatuses {
        regionName
        regionSlug
        lastScouted
        sourcesDue
        running
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
      freshnessDistribution {
        label
        count
      }
      confidenceDistribution {
        label
        count
      }
      unmetConcerns {
        title
        severity
        category
        opposing
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
  query AdminRegionSources($search: String) {
    adminRegionSources(search: $search) {
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

export const ADMIN_REGIONS = gql`
  query AdminRegions($leafOnly: Boolean, $limit: Int) {
    adminRegions(leafOnly: $leafOnly, limit: $limit) {
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

export const ADMIN_REGION = gql`
  query AdminRegion($id: String!) {
    adminRegion(id: $id) {
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
        locationName sourceUrl extractedAt contentDate sourceDiversity causeHeat channelDiversity
        startsAt endsAt actionUrl organizer isRecurring
        actors { id name actorType }
      }
      ... on GqlResourceSignal {
        id title summary sensitivity confidence location { lat lng precision }
        locationName sourceUrl extractedAt contentDate sourceDiversity causeHeat channelDiversity
        actionUrl availability isOngoing
        actors { id name actorType }
      }
      ... on GqlHelpRequestSignal {
        id title summary sensitivity confidence location { lat lng precision }
        locationName sourceUrl extractedAt contentDate sourceDiversity causeHeat channelDiversity
        urgency whatNeeded actionUrl statedGoal
        actors { id name actorType }
      }
      ... on GqlAnnouncementSignal {
        id title summary sensitivity confidence location { lat lng precision }
        locationName sourceUrl extractedAt contentDate sourceDiversity causeHeat channelDiversity
        severity subject effectiveDate sourceAuthority
        actors { id name actorType }
      }
      ... on GqlConcernSignal {
        id title summary sensitivity confidence location { lat lng precision }
        locationName sourceUrl extractedAt contentDate sourceDiversity causeHeat channelDiversity
        severity subject opposing
        actors { id name actorType }
      }
    }
  }
`;


export const SIGNALS_RECENT = gql`
  query SignalsRecent($limit: Int, $types: [SignalType!]) {
    signalsRecent(limit: $limit, types: $types) {
      ${SIGNAL_FIELDS}
    }
  }
`;

export const SIGNALS_WITHOUT_LOCATION = gql`
  query SignalsWithoutLocation($limit: Int) {
    signalsWithoutLocation(limit: $limit) {
      ${SIGNAL_FIELDS}
    }
  }
`;

export const ADMIN_SIGNALS = gql`
  query AdminSignals($limit: Int, $status: String) {
    adminSignals(limit: $limit, status: $status) {
      ${SIGNAL_FIELDS}
    }
  }
`;

export const SIGNAL_DETAIL = gql`
  query Signal($id: UUID!, $scheduleFrom: DateTime!, $scheduleTo: DateTime!) {
    signal(id: $id) {
      ${SIGNAL_FIELDS}
      ... on GqlGatheringSignal {
        citations { id sourceUrl snippet relevance }
        actors { id name actorType }
        schedule {
          id rrule scheduleText dtstart dtend timezone
          occurrences(from: $scheduleFrom, to: $scheduleTo)
        }
      }
      ... on GqlResourceSignal {
        citations { id sourceUrl snippet relevance }
        actors { id name actorType }
        schedule {
          id rrule scheduleText dtstart dtend timezone
          occurrences(from: $scheduleFrom, to: $scheduleTo)
        }
      }
      ... on GqlHelpRequestSignal {
        citations { id sourceUrl snippet relevance }
        actors { id name actorType }
      }
      ... on GqlAnnouncementSignal {
        citations { id sourceUrl snippet relevance }
        actors { id name actorType }
      }
      ... on GqlConcernSignal {
        citations { id sourceUrl snippet relevance }
        actors { id name actorType }
      }
    }
  }
`;

export const ACTORS_IN_BOUNDS = gql`
  query ActorsInBounds(
    $minLat: Float!, $maxLat: Float!,
    $minLng: Float!, $maxLng: Float!,
    $limit: Int
  ) {
    actorsInBounds(
      minLat: $minLat, maxLat: $maxLat,
      minLng: $minLng, maxLng: $maxLng,
      limit: $limit
    ) {
      id
      name
      actorType
      description
      signalCount
      locationName
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
  query AdminScoutRuns($region: String, $limit: Int) {
    adminScoutRuns(region: $region, limit: $limit) {
      runId
      region
      regionId
      flowType
      sources { id label }
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
        handlerFailures
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
        handlerFailures
      }
    }
  }
`;

export const ADMIN_SCOUT_RUN_EVENTS = gql`
  query AdminScoutRunEvents($runId: String!, $eventTypeFilter: String) {
    adminScoutRunEvents(runId: $runId, eventTypeFilter: $eventTypeFilter) {
      id
      parentId
      seq
      ts
      type
      sourceUrl
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
      reason
      strategy
      field
      oldValue
      newValue
      signalCount
      summary
    }
  }
`;

export const ADMIN_SCOUT_RUN_OUTCOMES = gql`
  query AdminScoutRunOutcomes($runId: String!) {
    adminScoutRunOutcomes(runId: $runId) {
      sourcesScraped(limit: 100) {
        items { canonicalKey url signalsProduced }
        total
      }
      signalsCreated(limit: 100) {
        items { nodeId nodeType title confidence sourceUrl }
        total
      }
      dedupMatches(limit: 50) {
        items { nodeType similarity existingId title }
        total
      }
      rejections(limit: 50) {
        items { title reason }
        total
      }
      sourcesDiscovered(limit: 50) {
        items { canonicalKey url discoveryMethod gapContext }
        total
      }
      expansionQueries(limit: 50) {
        items { query sourceUrl }
        total
      }
      failures(limit: 50) {
        items { handlerId error url variant }
        total
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

export const ADMIN_REGION_SOURCES_BY_REGION = gql`
  query AdminRegionSourcesByRegion($regionId: String!) {
    adminRegionSourcesByRegion(regionId: $regionId) {
      id
      url
      canonicalValue
      sourceLabel
      weight
      effectiveWeight
      discoveryMethod
      lastScraped
      signalsProduced
      active
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
      fetchCount
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
      fetchCount
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
      fetchCount
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
      fetchCount
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
      fetchCount
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
      fetchCount
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

// --- Source detail ---

export const SOURCE_DETAIL = gql`
  query SourceDetail($id: UUID!) {
    sourceDetail(id: $id) {
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
      signalsCorroborated
      consecutiveEmptyRuns
      active
      gapContext
      scrapeCount
      avgSignalsPerScrape
      sourceRole
      createdAt
      lastProducedSignal
      signals {
        id
        title
        signalType
        confidence
        extractedAt
        sourceUrl
      }
      archiveSummary {
        posts
        pages
        feeds
        shortVideos
        longVideos
        stories
        searchResults
        files
        lastFetchedAt
      }
      discoveryTree {
        nodes {
          id
          canonicalValue
          discoveryMethod
          active
          signalsProduced
        }
        edges {
          childId
          parentId
        }
        rootId
      }
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

export const GRAPH_NEIGHBORHOOD = gql`
  query GraphNeighborhood(
    $minLat: Float, $maxLat: Float, $minLng: Float, $maxLng: Float,
    $from: DateTime!, $to: DateTime!,
    $nodeTypes: [String!]!,
    $limit: Int!
  ) {
    graphNeighborhood(
      minLat: $minLat, maxLat: $maxLat, minLng: $minLng, maxLng: $maxLng,
      from: $from, to: $to, nodeTypes: $nodeTypes, limit: $limit
    ) {
      nodes {
        id
        nodeType
        label
        lat
        lng
        confidence
        metadata
      }
      edges {
        sourceId
        targetId
        edgeType
      }
      totalCount
    }
  }
`;

export const ADMIN_NODE_EVENTS = gql`
  query AdminNodeEvents($nodeId: String!, $limit: Int) {
    adminNodeEvents(nodeId: $nodeId, limit: $limit) {
      id
      parentId
      seq
      ts
      type
      sourceUrl
      query
      url
      provider
      platform
      signalType
      title
      resultCount
      confidence
      success
      action
      nodeId
      matchedId
      existingId
      spentCents
      reason
      field
      oldValue
      newValue
      summary
      similarity
      newSourceUrl
    }
  }
`;

// --- Event browser queries ---

export const ADMIN_EVENTS = gql`
  query AdminEvents(
    $limit: Int!
    $cursor: Int
    $search: String
    $from: DateTime
    $to: DateTime
    $runId: String
  ) {
    adminEvents(
      limit: $limit
      cursor: $cursor
      search: $search
      from: $from
      to: $to
      runId: $runId
    ) {
      events {
        seq
        ts
        type
        name
        layer
        id
        parentId
        correlationId
        runId
        handlerId
        summary
        payload
      }
      nextCursor
    }
  }
`;

export const ADMIN_CAUSAL_TREE = gql`
  query AdminCausalTree($seq: Int!) {
    adminCausalTree(seq: $seq) {
      events {
        seq
        ts
        type
        name
        layer
        id
        parentId
        handlerId
        summary
        payload
      }
      rootSeq
    }
  }
`;

export const ADMIN_CAUSAL_FLOW = gql`
  query AdminCausalFlow($runId: String!) {
    adminCausalFlow(runId: $runId) {
      events {
        seq
        ts
        type
        name
        layer
        id
        parentId
        correlationId
        runId
        handlerId
        summary
        payload
      }
    }
  }
`;

export const EVENTS_SUBSCRIPTION = gql`
  subscription Events($lastSeq: Int) {
    events(lastSeq: $lastSeq) {
      seq
      ts
      type
      name
      layer
      id
      parentId
      correlationId
      runId
      handlerId
      summary
      payload
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

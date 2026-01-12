import React, { useEffect, useState } from 'react';
import { getCacheMetrics, CacheMetrics } from '../../services/cacheService';
import { Activity, TrendingUp, Database } from 'lucide-react';

export const CacheMetricsCard: React.FC = () => {
  const [metrics, setMetrics] = useState<CacheMetrics | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const fetchMetrics = async () => {
      try {
        const data = await getCacheMetrics();
        setMetrics(data);
        setError(null);
      } catch (err) {
        console.error('Failed to fetch cache metrics:', err);
        setError('Failed to load cache metrics');
      } finally {
        setLoading(false);
      }
    };

    fetchMetrics();
    const interval = setInterval(fetchMetrics, 5000); // Refresh every 5 seconds

    return () => clearInterval(interval);
  }, []);

  if (loading) {
    return (
      <div className="flex items-center justify-center p-8">
        <span className="loading loading-spinner loading-lg"></span>
      </div>
    );
  }

  if (error || !metrics) {
    return (
      <div className="alert alert-error">
        <span>{error || 'No cache metrics available'}</span>
      </div>
    );
  }

  const totalRequests = metrics.hit_count + metrics.miss_count;
  const hitRatePercent = (metrics.hit_rate * 100).toFixed(1);
  const costSaved = metrics.cost_savings.total_saved.toFixed(2);
  const memoryMB = (metrics.performance.memory_usage / 1024 / 1024).toFixed(1);

  return (
    <div className="grid grid-cols-1 md:grid-cols-3 gap-4 mb-6">
      {/* Hit Rate Card */}
      <div className="stat bg-base-200 rounded-lg shadow-lg p-4">
        <div className="stat-figure text-success">
          <Activity size={32} />
        </div>
        <div className="stat-title">Cache Hit Rate</div>
        <div className="stat-value text-success">{hitRatePercent}%</div>
        <div className="stat-desc">
          {metrics.hit_count} hits / {totalRequests} requests
        </div>
      </div>

      {/* Cost Savings Card */}
      <div className="stat bg-base-200 rounded-lg shadow-lg p-4">
        <div className="stat-figure text-primary">
          <TrendingUp size={32} />
        </div>
        <div className="stat-title">Cost Savings</div>
        <div className="stat-value text-primary">${costSaved}</div>
        <div className="stat-desc">
          {metrics.hit_count} API calls avoided
        </div>
      </div>

      {/* Cache Size Card */}
      <div className="stat bg-base-200 rounded-lg shadow-lg p-4">
        <div className="stat-figure text-info">
          <Database size={32} />
        </div>
        <div className="stat-title">Cache Size</div>
        <div className="stat-value text-info">{memoryMB} MB</div>
        <div className="stat-desc">
          {metrics.performance.total_operations} operations
        </div>
      </div>
    </div>
  );
};
